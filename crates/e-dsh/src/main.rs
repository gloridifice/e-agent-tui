#![deny(clippy::significant_drop_in_scrutinee)]

//! e — terminal client for DeepSeek Harness (the `dshe` executable).
//!
//! One tokio runtime: a websocket reader forwards bridge messages, a writer
//! drains outbound commands, and an event-driven main loop selects terminal
//! input, bounded inbound batches, animation deadlines, and frame deadlines.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use e::model::{animation_active, tick_spinners, AppState};
use e::profile::{FrameMetrics, FrameSample};
use e::protocol::{ClientMessage, MAX_WIRE_FRAME_BYTES, WIRE_PROTOCOL_VERSION};
use e::runtime::{BridgeUiState, DrawPriority, EffectResult, RuntimeController, UiAction};
use e::runtime_ports::{
    BridgeTransportPort, ProductionRuntimePorts, ProductionTerminalEvents, TerminalEventPort,
    TerminalLifecyclePort, UiActionPorts,
};
use e::terminal_runtime::TerminalOwner;
use e_tui::ui::{render_with_cursor, TerminalSize};

const DSH_SERVER_CLOSED_MESSAGE: &str = "dsh 服务器已关闭。";
const INTERACTIVE_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const CONTENT_FRAME_INTERVAL: Duration = Duration::from_millis(30);
const MIN_ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const INBOUND_BATCH_LIMIT: usize = 64;
const INBOUND_BATCH_BUDGET: Duration = Duration::from_millis(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirtyReason {
    Interactive,
    Content,
    Animation,
}

#[derive(Debug)]
struct FrameScheduler {
    deadline: Option<Instant>,
    requested_at: Option<Instant>,
    last_frame: Option<Instant>,
}

impl FrameScheduler {
    fn new(now: Instant) -> Self {
        Self {
            deadline: Some(now),
            requested_at: Some(now),
            last_frame: None,
        }
    }

    fn interval(reason: DirtyReason) -> Duration {
        match reason {
            DirtyReason::Interactive => INTERACTIVE_FRAME_INTERVAL,
            DirtyReason::Content => CONTENT_FRAME_INTERVAL,
            DirtyReason::Animation => INTERACTIVE_FRAME_INTERVAL,
        }
    }

    fn request(&mut self, reason: DirtyReason, now: Instant) {
        let due = self
            .last_frame
            .map(|last| (last + Self::interval(reason)).max(now))
            .unwrap_or(now);
        self.deadline = Some(self.deadline.map_or(due, |current| current.min(due)));
        self.requested_at = Some(
            self.requested_at
                .map_or(now, |requested| requested.min(now)),
        );
    }

    fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    fn take_due(&mut self, now: Instant) -> Option<Instant> {
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.deadline = None;
            return self.requested_at.take();
        }
        None
    }

    fn complete(&mut self, now: Instant) {
        self.last_frame = Some(now);
    }
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending::<()>().await,
    }
}

fn animation_interval(state: &AppState) -> Duration {
    Duration::from_millis(
        state
            .config
            .spinner_frame_ms
            .max(MIN_ANIMATION_INTERVAL.as_millis() as u64),
    )
}

fn inbound_budget_remaining(count: usize, elapsed: Duration) -> bool {
    count < INBOUND_BATCH_LIMIT && elapsed < INBOUND_BATCH_BUDGET
}

/// A streamed assistant text/reasoning delta: render it as its own frame
/// instead of batching it with the rest of the inbound queue.
fn is_streaming_delta(event: &e_tui::AgentEvent) -> bool {
    matches!(
        event,
        e_tui::AgentEvent::Timeline(e_tui::agent::TimelineEvent::Append(record))
            if matches!(
                &record.fact,
                e_tui::agent::TimelineFact::AssistantChunk {
                    text,
                    reasoning,
                    ..
                } if !text.is_empty() || !reasoning.is_empty()
            )
    )
}

fn token_path() -> PathBuf {
    e::launcher::dsh_home().join("dsh-tui.token")
}

fn read_token() -> anyhow::Result<String> {
    let path = token_path();
    // The token is written by the bridge on startup; a freshly spawned dsh
    // may race the file slightly behind the listening socket, so retry a beat.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match std::fs::read_to_string(&path) {
            Ok(raw) => return Ok(raw.trim().to_string()),
            Err(error) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let _ = error;
            }
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "cannot read bridge token at {} ({error}) — is DSH running with the tui bridge?",
                    path.display()
                ));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliAction {
    Setup,
    Clean,
    Run {
        url: String,
        resume_session_id: Option<String>,
    },
}

fn parse_cli() -> anyhow::Result<CliAction> {
    parse_cli_from(std::env::args().skip(1))
}

fn parse_cli_from(mut args: impl Iterator<Item = String>) -> anyhow::Result<CliAction> {
    let Some(first) = args.next() else {
        return Ok(CliAction::Run {
            url: "ws://127.0.0.1:3080/dsh-tui".to_string(),
            resume_session_id: None,
        });
    };
    match first.as_str() {
        "setup" => {
            if args.next().is_some() {
                bail!("`dshe setup` takes no arguments. Run `dshe setup` alone.");
            }
            Ok(CliAction::Setup)
        }
        "clean" => {
            if args.next().is_some() {
                bail!("`dshe clean` takes no arguments. Run `dshe clean` alone.");
            }
            Ok(CliAction::Clean)
        }
        "install" => bail!("Unknown command `install`. Run `dshe setup` instead."),
        url => Ok(CliAction::Run {
            url: url.to_string(),
            resume_session_id: args.next(),
        }),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match parse_cli()? {
        CliAction::Setup => run_setup(),
        CliAction::Clean => run_clean(),
        CliAction::Run {
            url,
            resume_session_id,
        } => run_tui(url, resume_session_id).await,
    }
}

fn run_setup() -> anyhow::Result<()> {
    let home = e::launcher::dsh_home();
    e::setup::run_setup(&home).map_err(|error| anyhow::anyhow!("{error}"))
}

fn run_clean() -> anyhow::Result<()> {
    let home = e::launcher::dsh_home();
    let path = e::launcher::lock_path(&home);
    match e::launcher::clean(&home)? {
        e::launcher::CleanOutcome::NothingToClean => println!("Nothing to clean."),
        e::launcher::CleanOutcome::RemovedStaleLock { pid } => match pid {
            Some(pid) => println!(
                "Removed stale lock for managed DSH process {pid}: {}",
                path.display()
            ),
            None => println!("Removed stale lock: {}", path.display()),
        },
        e::launcher::CleanOutcome::StoppedManagedService { pid } => println!(
            "Stopped managed DSH process {pid} and removed lock: {}",
            path.display()
        ),
    }
    Ok(())
}

async fn run_tui(url: String, resume_session_id: Option<String>) -> anyhow::Result<()> {
    let mut phases = e::profile::PhaseTimers::new();
    // Tracy: active only with `--features tracy` AND DSH_TUI_TRACY=1.
    #[allow(unused_variables)]
    let _tracy = e::profile::start_tracy();

    let home = e::launcher::dsh_home();
    e::setup::require_ready(&home).map_err(|error| anyhow::anyhow!("{error}"))?;

    // Launcher preamble: ensure a DSH bridge is listening at `url`, spawning
    // `dsh --profile dshe` when none is (global dsh, else npx). `dsh_session`
    // records whether this process owns the spawned service so `release`
    // below can shut it down when the last TUI closes.
    let mut dsh_session = e::launcher::acquire(&url, &home)?;

    let _z = e::tracy_zone!("read_token");
    let token = read_token();
    drop(_z);
    phases.mark("read token");

    // Always release launcher ownership after a successful acquire, including
    // token-read and connection failures before the TUI has started.
    let result = match token {
        Ok(token) => run(url, token, resume_session_id, &mut phases).await,
        Err(error) => Err(error),
    };

    // On TUI exit: release the launcher bookkeeping. A dshe-spawned service
    // is shut down when this is the last attached TUI; an out-of-band dsh is
    // left untouched. Delay the confirmation until after leaving the alternate
    // screen so it remains visible in the caller's terminal.
    let dsh_server_closed = e::launcher::release(&mut dsh_session);

    if dsh_server_closed {
        println!("{DSH_SERVER_CLOSED_MESSAGE}");
    }
    result
}

#[derive(Default)]
struct EffectExecution {
    completed: Vec<EffectResult>,
    quit: bool,
    fatal: Option<String>,
}

async fn execute_runtime_effects(
    effects: Vec<UiAction>,
    outbound: &impl BridgeTransportPort,
    scheduler: &mut FrameScheduler,
    ports: &mut impl UiActionPorts,
) -> EffectExecution {
    let mut execution = EffectExecution::default();
    for effect in effects {
        match effect {
            UiAction::Agent(request) => {
                let message = e::bridge::adapter::agent_request_to_client(request);
                if let Err(error) = outbound.send_message(message).await {
                    execution.fatal = Some(error);
                    break;
                }
            }
            UiAction::ResolvePreview(request) => {
                let result = ports.resolve_preview(request.clone()).await;
                execution.completed.push(EffectResult::PreviewResolved {
                    request_id: request.request_id,
                    key: request.key,
                    revision: request.revision,
                    result,
                });
            }
            UiAction::PersistConfig(config) => {
                execution
                    .completed
                    .push(EffectResult::ConfigPersisted(ports.persist_config(&config)));
            }
            UiAction::ReloadConfig => {
                execution.completed.push(match ports.load_config() {
                    Ok((config, themes)) => EffectResult::ConfigReloaded {
                        config: Box::new(config),
                        themes,
                    },
                    Err(error) => EffectResult::ConfigReloadFailed(error),
                });
            }
            UiAction::PersistSessionId(session_id) => {
                ports.persist_session_id(session_id);
            }
            UiAction::WriteClipboard(text) => {
                let lines = text.lines().count();
                let result = ports.write_clipboard(text);
                execution.completed.push(match result {
                    Ok(()) => EffectResult::ClipboardWritten { lines },
                    Err(error) => EffectResult::ClipboardFailed(error.to_string()),
                });
            }
            UiAction::RequestDraw(priority) => {
                let reason = match priority {
                    DrawPriority::Interactive => DirtyReason::Interactive,
                    DrawPriority::Content => DirtyReason::Content,
                    DrawPriority::Animation => DirtyReason::Animation,
                };
                scheduler.request(reason, ports.now());
            }
            UiAction::Quit => execution.quit = true,
            UiAction::Fatal(reason) => {
                execution.fatal = Some(reason);
                break;
            }
        }
    }
    execution
}

fn wire_frame_limit(legacy_megabytes: Option<&str>) -> usize {
    legacy_megabytes
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|mb| mb.checked_mul(1024 * 1024))
        .filter(|bytes| *bytes >= MAX_WIRE_FRAME_BYTES)
        .unwrap_or(MAX_WIRE_FRAME_BYTES)
}

async fn run(
    url: String,
    token: String,
    resume_session_id: Option<String>,
    phases: &mut e::profile::PhaseTimers,
) -> anyhow::Result<()> {
    // Config: persisted TOML, live-editable via /settings (D26–D30).
    let _z = e::tracy_zone!("config load");
    let mut config = e::config::load();
    // Discover the themes directory (ensuring the two defaults exist) and
    // resolve the configured theme name to a palette. `themes` is refreshed
    // by `/reload` and `/theme`; `config.resolved_theme` caches the result
    // so render-time lookups never touch disk.
    let mut themes = e::theme::load_themes(&e::config::themes_dir());
    config.resolved_theme = e::theme::resolve(&config.theme, &themes);
    // A fresh process opens a NEW session by default (each process shows
    // one session); resume only through the explicit CLI session id or the
    // opt-in remember-last-session setting. The new session is created on
    // the configured default mode (the bridge falls back to `standard`
    // when that preset id is stale).
    let resume_session_id = resume_session_id.or_else(|| {
        if config.remember_last_session {
            e::config::StateFile::load().last_session_id
        } else {
            None
        }
    });
    drop(_z);
    phases.mark("config load");
    let theme = config.theme();
    let mut app = AppState::default();
    app.config = config.clone();
    app.interaction = e_tui::InteractionModel::new(&config);
    let state = Arc::new(std::sync::Mutex::new(app));
    let mut theme = theme;

    let max_frame_bytes =
        wire_frame_limit(std::env::var("DSHE_LEGACY_MAX_FRAME_MB").ok().as_deref());
    let launch_cwd = std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    let hello = ClientMessage::Hello {
        token,
        resume_session_id: resume_session_id.clone(),
        cwd: launch_cwd,
        mode: if resume_session_id.is_none() {
            Some(config.default_mode.clone())
        } else {
            None
        },
        protocol_version: WIRE_PROTOCOL_VERSION,
    };
    let _z = e::tracy_zone!("ws connect");
    let mut bridge_io = e::bridge_io::BridgeIo::connect(&url, hello, max_frame_bytes).await?;
    drop(_z);
    phases.mark("ws connect");
    phases.mark("hello sent");
    let tx_out = bridge_io.outbound.clone();
    let state_r = Arc::clone(&state);

    // ---- event-driven main loop ----
    let mut terminal = TerminalOwner::new().context("initialize terminal")?;
    phases.mark("terminal setup");
    let mut events = ProductionTerminalEvents::new();
    let mut runtime_ports = ProductionRuntimePorts;
    let mut scheduler = FrameScheduler::new(runtime_ports.now());
    let mut spinner_deadline: Option<Instant> = None;
    let mut frame_metrics = FrameMetrics::from_env();
    let mut pending_update_elapsed = Duration::ZERO;
    let mut fatal: Option<String> = None;
    let mut first_draw_done = false;

    'outer: loop {
        let _main_loop_zone = e::tracy_zone!("main loop");
        let mut pending_event = None;
        let mut first_inbound = None;
        let frame_deadline = scheduler.deadline();
        let reveal_deadline = state_r.lock().unwrap().reveal_deadline();
        let animation_deadline =
            e_tui::reveal::earliest_deadline(spinner_deadline, reveal_deadline);
        tokio::select! {
            maybe = bridge_io.inbound.recv() => {
                let Some(msg) = maybe else {
                    fatal = Some("bridge disconnected".into());
                    break 'outer;
                };
                first_inbound = Some(msg);
            }
            event = events.next_event() => {
                match event {
                    Some(Ok(event)) => {
                        pending_event = Some(event);
                        scheduler.request(DirtyReason::Interactive, Instant::now());
                    }
                    Some(Err(error)) => {
                        fatal = Some(format!("terminal event stream failed: {error}"));
                        break 'outer;
                    }
                    None => {
                        fatal = Some("terminal event stream closed".into());
                        break 'outer;
                    }
                }
            }
            _ = wait_for_deadline(frame_deadline) => {}
            _ = wait_for_deadline(animation_deadline) => {
                let now = Instant::now();
                let mut state = state_r.lock().unwrap();
                let spinner_due = spinner_deadline.is_some_and(|deadline| deadline <= now);
                let reveal_due = state.reveal_deadline().is_some_and(|deadline| deadline <= now);
                let mut redraw = false;
                if spinner_due {
                    redraw |= tick_spinners(&mut state, now);
                    spinner_deadline = animation_active(&state, now)
                        .then(|| now + animation_interval(&state));
                }
                if reveal_due {
                    redraw |= state.tick_reveals(now);
                }
                if redraw {
                    scheduler.request(DirtyReason::Animation, now);
                }
            }
        }

        if let Some(first) = first_inbound {
            let _batch_zone = e::tracy_zone!("inbound batch");
            let batch_started = Instant::now();
            let mut next = Some(first);
            let mut count = 0usize;
            while let Some(msg) = next.take() {
                count += 1;
                let event = e::bridge::adapter::normalize_server_message(msg);
                let is_snapshot = matches!(
                    &event,
                    e_tui::AgentEvent::Timeline(e_tui::agent::TimelineEvent::Snapshot { .. })
                );
                // Streaming text/reasoning deltas render frame-by-frame so the
                // assistant output (and Thinking reasoning) appears
                // incrementally instead of being swallowed by the inbound
                // batch and jumping in whole chunks.
                let streaming_delta = is_streaming_delta(&event);
                if is_snapshot {
                    phases.mark("snapshot received");
                }
                let update_started = Instant::now();
                let mut interaction = {
                    let mut app = state_r.lock().unwrap();
                    std::mem::take(&mut app.interaction)
                };
                let mut ui = BridgeUiState {
                    scroll: &mut interaction.scroll,
                    input: &mut interaction.input,
                    input_page: &mut interaction.input_page,
                    approval: &mut interaction.approval,
                    question: &mut interaction.question,
                    queue: &mut interaction.queue,
                };
                let effects = RuntimeController::apply_agent(event, &state_r, &mut ui);
                state_r.lock().unwrap().interaction = interaction;
                let execution =
                    execute_runtime_effects(effects, &tx_out, &mut scheduler, &mut runtime_ports)
                        .await;
                if let Some(reason) = execution.fatal {
                    fatal = Some(reason);
                    break 'outer;
                }
                if execution.quit {
                    break 'outer;
                }
                pending_update_elapsed += update_started.elapsed();
                if is_snapshot {
                    phases.mark("snapshot applied");
                }
                if streaming_delta {
                    // Hand the render loop back after every delta so each
                    // streamed increment becomes its own visible frame.
                    break;
                }
                if !inbound_budget_remaining(count, batch_started.elapsed()) {
                    break;
                }
                next = match bridge_io.inbound.try_recv() {
                    Ok(msg) => Some(msg),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        fatal = Some("bridge disconnected".into());
                        break 'outer;
                    }
                };
            }
            scheduler.request(DirtyReason::Content, Instant::now());
        }

        // ---- queued prompts: auto-dispatch the next one now that the agent
        // ---- is idle (one at a time — each dispatch keeps it busy again).
        let queued_effects = RuntimeController::dispatch_next_queued(&state_r);
        if !queued_effects.is_empty() {
            let execution = execute_runtime_effects(
                queued_effects,
                &tx_out,
                &mut scheduler,
                &mut runtime_ports,
            )
            .await;
            if let Some(reason) = execution.fatal {
                fatal = Some(reason);
                break 'outer;
            }
            if execution.quit {
                break 'outer;
            }
            scheduler.request(DirtyReason::Content, Instant::now());
        }

        // ---- directly-woken terminal event ----
        if let Some(event) = pending_event {
            let focus = {
                let state = state_r.lock().unwrap();
                let drafting = state.is_new_conversation();
                e::runtime::TerminalFocus {
                    help_visible: state.interaction.help_visible,
                    input_page_open: state.interaction.input_page.is_some(),
                    approval_open: !drafting && state.interaction.approval.is_some(),
                    reading_view_open: state.reading.is_some(),
                }
            };
            let route = e::runtime::route_terminal_event(event, focus);
            let terminal_size = terminal.size();
            let terminal_height = terminal_size.as_ref().map(|s| s.height).unwrap_or(40);
            let terminal_width = terminal_size.as_ref().map(|s| s.width).unwrap_or(120);
            let mut interaction = {
                let mut app = state_r.lock().unwrap();
                std::mem::take(&mut app.interaction)
            };
            let effects = RuntimeController::apply_terminal_route(
                route,
                TerminalSize {
                    width: terminal_width,
                    height: terminal_height,
                },
                runtime_ports.now(),
                &state_r,
                &mut e::runtime::TerminalUiState {
                    scroll: &mut interaction.scroll,
                    input: &mut interaction.input,
                    input_page: &mut interaction.input_page,
                    help_visible: &mut interaction.help_visible,
                    copy_toast: &mut interaction.copy_toast,
                    approval: &mut interaction.approval,
                    question: &mut interaction.question,
                    queue: &mut interaction.queue,
                    config: &mut config,
                    themes: &mut themes,
                    theme: &mut theme,
                },
            );
            state_r.lock().unwrap().interaction = interaction;
            let execution =
                execute_runtime_effects(effects, &tx_out, &mut scheduler, &mut runtime_ports).await;
            for result in execution.completed {
                match result {
                    EffectResult::ClipboardWritten { lines } => {
                        state_r.lock().unwrap().interaction.copy_toast =
                            Some((format!("已复制 {lines} 行"), runtime_ports.now()));
                    }
                    EffectResult::ConfigReloaded {
                        config: loaded,
                        themes: loaded_themes,
                    } => {
                        let mut interaction = {
                            let mut app = state_r.lock().unwrap();
                            std::mem::take(&mut app.interaction)
                        };
                        RuntimeController::apply_reloaded_config(
                            *loaded,
                            loaded_themes,
                            &state_r,
                            &mut e::runtime::TerminalUiState {
                                scroll: &mut interaction.scroll,
                                input: &mut interaction.input,
                                input_page: &mut interaction.input_page,
                                help_visible: &mut interaction.help_visible,
                                copy_toast: &mut interaction.copy_toast,
                                approval: &mut interaction.approval,
                                question: &mut interaction.question,
                                queue: &mut interaction.queue,
                                config: &mut config,
                                themes: &mut themes,
                                theme: &mut theme,
                            },
                        );
                        state_r.lock().unwrap().interaction = interaction;
                    }
                    other => {
                        if RuntimeController::apply_effect_result(other, &state_r) {
                            scheduler.request(DirtyReason::Content, runtime_ports.now());
                        }
                    }
                }
            }
            if let Some(reason) = execution.fatal {
                fatal = Some(reason);
                break 'outer;
            }
            if execution.quit {
                break 'outer;
            }
        }

        // Start the spinner clock only while a running/settling indicator
        // exists. Reveal lanes contribute their own exact deadlines at the
        // next select turn; a fully idle client has no periodic wakeup.
        if spinner_deadline.is_none() {
            let now = Instant::now();
            let state = state_r.lock().unwrap();
            if animation_active(&state, now) {
                spinner_deadline = Some(now);
            }
        }

        let now = Instant::now();
        if let Some(requested_at) = scheduler.take_due(now) {
            let mut interaction = {
                let mut app = state_r.lock().unwrap();
                std::mem::take(&mut app.interaction)
            };
            let mut state = state_r.lock().unwrap();
            let toast = interaction.copy_toast.as_ref().map(|(t, _)| t.as_str());
            let first_frame = !first_draw_done;
            let _first_zone = if first_frame {
                e::tracy_zone!("first frame")
            } else {
                None
            };
            let transaction = terminal.draw(|frame| {
                render_with_cursor(
                    frame,
                    &mut state,
                    &interaction.input,
                    &mut interaction.scroll,
                    &theme,
                    e_tui::ui::RenderOverlays {
                        help_visible: interaction.help_visible,
                        toast,
                        input_page: interaction.input_page.as_mut(),
                        settings: None,
                        login: None,
                        approval: interaction.approval.as_ref(),
                        queue: &interaction.queue,
                    },
                )
            });
            let cache_work = state.render.transcript_cache.take_work_stats();
            let preview_work = state.preview.take_work_stats();
            drop(state);
            state_r.lock().unwrap().interaction = interaction;
            let transaction = transaction?;
            scheduler.complete(Instant::now());
            let io = transaction.io;
            let report = frame_metrics.record(FrameSample {
                scheduler_delay: now.saturating_duration_since(requested_at),
                update: pending_update_elapsed,
                render: transaction.render,
                draw: Duration::from_nanos(io.draw_ns),
                flush: Duration::from_nanos(io.flush_ns),
                total: transaction.total,
                changed_cells: io.changed_cells,
                emitted_bytes: io.emitted_bytes,
                cache_rebuilds: cache_work.rebuilds,
                cache_patches: cache_work.patches,
                materialized_rows: cache_work.materialized_rows,
                preview_rebuilds: preview_work.rebuilds,
                preview_patches: preview_work.patches,
                preview_materialized_rows: preview_work.materialized_rows,
            });
            pending_update_elapsed = Duration::ZERO;
            if let Some(report) = report {
                eprintln!("{report}");
            }
            if first_frame {
                drop(_first_zone);
                first_draw_done = true;
                phases.mark("first frame");
            }
        }
    }

    bridge_io.shutdown();
    terminal.restore_terminal().ok();
    if let Some(reason) = fatal {
        bail!("{reason}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_confirmation_has_the_required_text() {
        assert_eq!(DSH_SERVER_CLOSED_MESSAGE, "dsh 服务器已关闭。");
    }

    fn args<'a>(items: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
        items.iter().map(|s| s.to_string())
    }

    #[test]
    fn cli_routes_maintenance_commands_and_run() {
        assert_eq!(
            parse_cli_from(args(&[])).unwrap(),
            CliAction::Run {
                url: "ws://127.0.0.1:3080/dsh-tui".to_string(),
                resume_session_id: None,
            }
        );
        assert_eq!(parse_cli_from(args(&["setup"])).unwrap(), CliAction::Setup);
        assert_eq!(parse_cli_from(args(&["clean"])).unwrap(), CliAction::Clean);
        assert_eq!(
            parse_cli_from(args(&["ws://host/dsh-tui", "sess-1"])).unwrap(),
            CliAction::Run {
                url: "ws://host/dsh-tui".to_string(),
                resume_session_id: Some("sess-1".to_string()),
            }
        );
    }

    #[test]
    fn cli_rejects_install_and_maintenance_command_arguments() {
        let install = parse_cli_from(args(&["install"])).unwrap_err().to_string();
        assert!(install.contains("`dshe setup`"), "{install}");
        assert!(install.contains("install"));

        let extra = parse_cli_from(args(&["setup", "extra"]))
            .unwrap_err()
            .to_string();
        assert!(extra.contains("`dshe setup`"));
        assert!(extra.contains("no arguments"));

        let extra = parse_cli_from(args(&["clean", "extra"]))
            .unwrap_err()
            .to_string();
        assert!(extra.contains("`dshe clean`"));
        assert!(extra.contains("no arguments"));
    }

    #[test]
    fn normal_frames_are_bounded_and_legacy_growth_is_explicit() {
        assert_eq!(wire_frame_limit(None), MAX_WIRE_FRAME_BYTES);
        assert_eq!(wire_frame_limit(Some("64")), 64 * 1024 * 1024);
        assert_eq!(wire_frame_limit(Some("1")), MAX_WIRE_FRAME_BYTES);
        assert_eq!(wire_frame_limit(Some("not-a-number")), MAX_WIRE_FRAME_BYTES);
    }

    #[test]
    fn queued_dispatch_releases_the_state_lock() {
        let state = std::sync::Mutex::new(AppState::default());
        state.lock().unwrap().interaction.queue.push("next".into());

        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(e_tui::AgentRequest::Input { text })] if text == "next"
        ));
        let guard = state
            .try_lock()
            .expect("dispatch must not retain the mutex guard");
        assert!(guard.session.working);
        assert!(guard.interaction.queue.is_empty());
    }

    #[tokio::test]
    async fn preview_executor_returns_owned_completion_without_ui_state() {
        let now = Instant::now();
        let mut ports = ProductionRuntimePorts;
        let (transport, _receiver) = tokio::sync::mpsc::channel(1);
        let mut scheduler = FrameScheduler::new(now);
        let request = e_tui::PreviewRequest {
            request_id: e_tui::PreviewRequestId(7),
            key: e_tui::PreviewKey("unsupported:test".into()),
            revision: e_tui::PreviewRevision(3),
        };
        let execution = execute_runtime_effects(
            vec![UiAction::ResolvePreview(request.clone())],
            &transport,
            &mut scheduler,
            &mut ports,
        )
        .await;
        assert!(matches!(
            execution.completed.as_slice(),
            [EffectResult::PreviewResolved { request_id, key, revision, result: Err(error) }]
                if *request_id == request.request_id
                    && *key == request.key
                    && *revision == request.revision
                    && error.contains("unsupported")
        ));
    }

    #[test]
    fn scheduler_is_idle_after_due_frame_completes() {
        let now = Instant::now();
        let mut scheduler = FrameScheduler::new(now);
        assert_eq!(scheduler.take_due(now), Some(now));
        scheduler.complete(now);
        assert_eq!(scheduler.deadline(), None);
    }

    #[test]
    fn interactive_request_preempts_content_deadline() {
        let now = Instant::now();
        let mut scheduler = FrameScheduler::new(now);
        scheduler.take_due(now);
        scheduler.complete(now);
        scheduler.request(DirtyReason::Content, now + Duration::from_millis(1));
        assert_eq!(scheduler.deadline(), Some(now + CONTENT_FRAME_INTERVAL));
        scheduler.request(DirtyReason::Interactive, now + Duration::from_millis(2));
        assert_eq!(scheduler.deadline(), Some(now + INTERACTIVE_FRAME_INTERVAL));
    }

    #[test]
    fn repeated_interaction_coalesces_at_one_deadline() {
        let now = Instant::now();
        let mut scheduler = FrameScheduler::new(now);
        scheduler.take_due(now);
        scheduler.complete(now);
        for millis in 1..10 {
            scheduler.request(
                DirtyReason::Interactive,
                now + Duration::from_millis(millis),
            );
        }
        assert_eq!(scheduler.deadline(), Some(now + INTERACTIVE_FRAME_INTERVAL));
    }

    #[test]
    fn inbound_batch_stops_on_count_or_time_budget() {
        assert!(inbound_budget_remaining(1, Duration::ZERO));
        assert!(!inbound_budget_remaining(
            INBOUND_BATCH_LIMIT,
            Duration::ZERO
        ));
        assert!(!inbound_budget_remaining(1, INBOUND_BATCH_BUDGET));
    }

    #[test]
    fn earliest_animation_deadline_keeps_reveal_and_spinner_independent() {
        use ratatui::text::Line;

        let now = Instant::now();
        let spinner = now + Duration::from_millis(120);
        let mut state = AppState::default();
        state.config.message_chars_per_second = e_tui::config::RevealRate::new(16).unwrap();
        state.config.preview_lines_per_second = e_tui::config::RevealRate::new(32).unwrap();
        let mut transcript = e_tui::reveal::RevealTrack::default();
        transcript.reconcile(
            e_tui::reveal::RevealSignature::from_lines(&[Line::from("abc")]),
            true,
            now,
            16,
        );
        state.render.transcript_reveals.insert(
            e_tui::display::DisplayId::correlated("assistant", "deadline"),
            transcript,
        );
        let mut preview = e_tui::reveal::LineRevealTrack::default();
        preview.reconcile(&[Line::from("a"), Line::from("b")], now, 32);
        state.preview.reveal = Some(preview);

        let first_fade = now + Duration::from_millis(16);
        assert_eq!(state.reveal_deadline(), Some(first_fade));
        assert_eq!(
            e_tui::reveal::earliest_deadline(Some(spinner), state.reveal_deadline()),
            Some(first_fade)
        );
        assert!(state.tick_reveals(first_fade));
        let preview_due = now + e_tui::reveal::reveal_interval(32);
        assert_eq!(state.reveal_deadline(), Some(preview_due));
        assert!(state.tick_reveals(preview_due));
        assert_eq!(
            state
                .render
                .transcript_reveals
                .values()
                .next()
                .map(e_tui::reveal::RevealTrack::revealed),
            Some(1),
            "16/s transcript lane is not due at the 32/s Preview deadline"
        );
        assert_eq!(
            state
                .preview
                .reveal
                .as_ref()
                .map(e_tui::reveal::LineRevealTrack::revealed),
            Some(2)
        );
    }

    #[test]
    fn animation_interval_honors_config_with_safe_floor() {
        let mut state = AppState::default();
        state.config.spinner_frame_ms = 120;
        assert_eq!(animation_interval(&state), Duration::from_millis(120));
        state.config.spinner_frame_ms = 0;
        assert_eq!(animation_interval(&state), MIN_ANIMATION_INTERVAL);
    }
}
