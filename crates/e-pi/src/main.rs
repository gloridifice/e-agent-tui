#![deny(clippy::significant_drop_in_scrutinee)]

//! `pie` — the Pi coding agent hosted by the kernel-neutral ratatui frontend.
//!
//! Pi remains the authoritative agent runtime. This process owns only the Pi
//! RPC child, protocol adaptation, terminal lifecycle, and frontend runtime.

mod platform_input;
mod workspace_runner;
use workspace_runner::run_frontend;

#[cfg(test)]
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context};
#[cfg(test)]
use e_pi::{
    adapter::AUTH_REFRESH_COMMAND, auth::AuthAction, protocol::RpcCommand,
    session_worker::route_auth_action,
};
use e_pi::{
    herdr::{Reporter as HerdrReporter, Status as HerdrStatus},
    process::{PiLaunchOptions, ProjectTrust},
    session_worker::{SessionWorker, SessionWorkerEvent},
};
use e_tui::runtime::sessions::SessionWorkspace;
use e_tui::runtime::{
    animation_active, animation_interval, execute_ui_actions, route_terminal_event, tick_spinners,
    wait_for_deadline, AgentRequestPort, DirtyReason, FrameScheduler, ProductionTerminalEvents,
    RuntimeController, RuntimeState, RuntimeUiState, TerminalEventPort, TerminalFocus,
    TerminalLifecyclePort, TerminalOwner, TerminalUiState, UiActionPorts,
};
use e_tui::{
    ui::TerminalSize, AgentRequest, Config, EffectResult, PreviewContent, PreviewRequest, ThemeFile,
};

#[derive(Debug, Clone)]
struct Cli {
    launch: PiLaunchOptions,
    resume: Option<String>,
}

fn help() -> &'static str {
    "pie [--cwd <directory>] [--session <file> | --resume <session_id>] [--approve|--no-approve] [--pi <executable>]\n\nRuns the e-tui frontend against the official `pi --mode rpc` runtime.\n  -r, --resume <session_id>  Resume a saved session by exact ID in its saved workspace.\nProject trust follows native Pi behavior unless an explicit trust flag is supplied.\nOn exit, prints a command to resume the current saved session."
}

fn parse_cli_from(mut args: impl Iterator<Item = String>) -> anyhow::Result<Cli> {
    let mut launch = PiLaunchOptions::for_cwd(std::env::current_dir()?);
    let mut resume = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--cwd" => {
                launch.cwd = PathBuf::from(args.next().context("--cwd requires a directory")?)
            }
            "--session" | "--resume" | "-r" => {
                if launch.session.is_some() || resume.is_some() {
                    bail!("specify only one session: --session <file> or --resume <session_id>");
                }
                let value = args
                    .next()
                    .filter(|value| !value.trim().is_empty() && !value.starts_with('-'))
                    .with_context(|| {
                        if argument == "--session" {
                            "--session requires a Pi session file".to_owned()
                        } else {
                            format!("{argument} requires a Pi session ID")
                        }
                    })?;
                if argument == "--session" {
                    launch.session = Some(value);
                } else {
                    resume = Some(value);
                }
            }
            "--approve" => launch.trust = ProjectTrust::Approve,
            "--no-approve" => launch.trust = ProjectTrust::Reject,
            "--pi" => launch.executable = args.next().context("--pi requires an executable")?,
            "--help" | "-h" => {
                println!("{}", help());
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("pie {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            option if option.starts_with('-') => {
                bail!("unknown pie option `{option}`\n\n{}", help())
            }
            session if launch.session.is_none() && resume.is_none() => {
                launch.session = Some(session.to_owned())
            }
            extra => bail!("unexpected argument `{extra}`\n\n{}", help()),
        }
    }
    Ok(Cli { launch, resume })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Cli { mut launch, resume } = parse_cli_from(std::env::args().skip(1))?;
    if let Some(id) = resume {
        let session = tokio::task::spawn_blocking(move || {
            e_pi::session_index::SessionIndex::enumerate(&e_pi::session_index::session_root())
                .resolve_id(&id)
        })
        .await
        .context("resolve Pi session ID")?
        .map_err(anyhow::Error::msg)?;
        launch.cwd = session.cwd;
        launch.session = Some(session.path.to_string_lossy().into_owned());
    }
    run(launch).await
}

fn resume_command(session_path: &str) -> Option<String> {
    let session =
        e_pi::session_index::read_saved_session(std::path::Path::new(session_path)).ok()?;
    let root = std::path::absolute(e_pi::session_index::session_root()).ok()?;
    if session.path.starts_with(root)
        && session
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && !session.id.starts_with('-')
    {
        return Some(format!("pie --resume {}", session.id));
    }
    let path = session.path.to_string_lossy();
    #[cfg(windows)]
    let quoted = path.replace('\'', "''");
    #[cfg(not(windows))]
    let quoted = path.replace('\'', "'\"'\"'");
    Some(format!("pie --session '{quoted}'"))
}

struct PiRuntimePorts {
    persist_session_enabled: bool,
    history: Arc<std::sync::Mutex<e_pi::execution_history_store::HistoryRecorder>>,
    shell: e_pi::shell::ShellRunner,
}

impl UiActionPorts for PiRuntimePorts {
    fn start_shell(&mut self, request: e_tui::shell::ShellRequest) -> Result<(), String> {
        self.shell.start(request)
    }
    fn cancel_shell(&mut self, id: &e_tui::display::DisplayId) {
        self.shell.cancel(id);
    }
    async fn validate_links(
        &mut self,
        request: &e_tui::link_copy::LinkValidationRequest,
    ) -> Vec<e_tui::link_copy::CandidateGroupValidation> {
        let request = request.clone();
        tokio::task::spawn_blocking(move || e_pi::path_completion::validate_links(&request))
            .await
            .unwrap_or_default()
    }
    async fn complete_paths(
        &mut self,
        request: &e_tui::path_completion::PathCompletionRequest,
    ) -> Vec<e_tui::path_completion::PathCandidate> {
        let request = request.clone();
        tokio::task::spawn_blocking(move || e_pi::path_completion::complete(&request))
            .await
            .unwrap_or_default()
    }
    fn query_history(
        &mut self,
        request: e_tui::execution_history::HistoryQueryRequest,
    ) -> impl std::future::Future<
        Output = Result<e_tui::execution_history::HistoryQueryResult, String>,
    > + Send {
        let history = Arc::clone(&self.history);
        async move {
            tokio::task::spawn_blocking(move || history.lock().unwrap().query(&request))
                .await
                .map_err(|error| format!("execution-history query worker failed: {error}"))?
        }
    }

    fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String> {
        let mut config = e_pi::config::load();
        let themes = e_pi::effects::load_themes(&e_pi::config::themes_dir());
        config.resolved_theme = e_tui::theme::resolve(&config.theme, &themes);
        Ok((config, themes))
    }

    fn persist_config(&mut self, config: &Config) -> Result<(), String> {
        e_pi::config::save(config)
    }

    fn persist_session_id(&mut self, session_id: String) {
        if !self.persist_session_enabled {
            return;
        }
        let mut state = e_pi::config::StateFile::load();
        state.last_session_path = Some(session_id);
        state.save();
    }

    fn read_clipboard(&mut self) -> Result<e_tui::ClipboardPaste, String> {
        e_pi::effects::read_clipboard()
    }

    fn write_clipboard(&mut self, text: String) -> Result<(), String> {
        e_pi::effects::write_clipboard(text)
    }

    fn resolve_preview(
        &mut self,
        request: PreviewRequest,
    ) -> impl std::future::Future<Output = Result<PreviewContent, String>> + Send {
        e_pi::effects::resolve_preview(request)
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

struct PiAgentPort<'a> {
    requests: &'a tokio::sync::mpsc::Sender<AgentRequest>,
}

impl AgentRequestPort for PiAgentPort<'_> {
    async fn send_agent_request(&mut self, request: AgentRequest) -> Result<(), String> {
        self.requests
            .try_send(request)
            .map_err(|error| format!("Pi request could not be admitted: {error}"))
    }
}

async fn run(launch: PiLaunchOptions) -> anyhow::Result<()> {
    let herdr = HerdrReporter::from_env();
    let result = run_frontend(launch, herdr.as_ref()).await;
    if let Some(herdr) = herdr {
        herdr.shutdown().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<'a>(items: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
        items.iter().map(|item| (*item).to_owned())
    }

    #[test]
    fn cli_defaults_to_native_trust_and_current_directory() {
        let cli = parse_cli_from(args(&[])).unwrap();
        assert_eq!(cli.launch.trust, ProjectTrust::Native);
        assert!(cli.launch.session.is_none());
    }

    #[tokio::test]
    async fn auth_refresh_control_uses_correlated_json_arguments() {
        let (rpc, mut commands) = tokio::sync::mpsc::channel(1);
        let mut pending = VecDeque::new();
        route_auth_action(
            AuthAction::RefreshRuntime {
                flow_id: "flow-1".into(),
                provider: "provider with spaces".into(),
            },
            &rpc,
            &mut pending,
        )
        .await
        .unwrap();
        let RpcCommand::Prompt { message, .. } = commands.recv().await.unwrap() else {
            panic!("expected refresh prompt")
        };
        let payload = message
            .strip_prefix(&format!("/{AUTH_REFRESH_COMMAND} "))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(payload).unwrap(),
            serde_json::json!({
                "flowId": "flow-1",
                "provider": "provider with spaces",
            })
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn cli_accepts_explicit_session_cwd_and_trust() {
        let cli = parse_cli_from(args(&[
            "--cwd",
            "project",
            "--session",
            "one.jsonl",
            "--approve",
        ]))
        .unwrap();
        assert_eq!(cli.launch.cwd, PathBuf::from("project"));
        assert_eq!(cli.launch.session.as_deref(), Some("one.jsonl"));
        assert_eq!(cli.launch.trust, ProjectTrust::Approve);
    }
}
