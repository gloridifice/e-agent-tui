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
use e::config::Config;
use e::copy;
use e::input::InputState;
use e::input_page::InputPageSession;
use e::model::{animation_active, tick_spinners, AppState};
use e::profile::{FrameMetrics, FrameSample};
use e::protocol::{ClientMessage, ServerMessage, MAX_WIRE_FRAME_BYTES, WIRE_PROTOCOL_VERSION};
use e::runtime::{BridgeUiState, DrawPriority, EffectResult, RuntimeController, RuntimeEffect};
use e::runtime_ports::{
    BridgeTransportPort, ProductionRuntimePorts, ProductionTerminalEvents, RuntimeEffectPorts,
    TerminalEventPort, TerminalLifecyclePort,
};
use e::terminal_runtime::TerminalOwner;
use e::ui::{render_with_cursor, CopyOverlay, ScrollState};

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
    effects: Vec<RuntimeEffect>,
    outbound: &impl BridgeTransportPort,
    scheduler: &mut FrameScheduler,
    ports: &mut impl RuntimeEffectPorts,
) -> EffectExecution {
    let mut execution = EffectExecution::default();
    for effect in effects {
        match effect {
            RuntimeEffect::Send(message) => {
                if let Err(error) = outbound.send_message(message).await {
                    execution.fatal = Some(error);
                    break;
                }
            }
            RuntimeEffect::PersistConfig(config) => {
                execution
                    .completed
                    .push(EffectResult::ConfigPersisted(ports.persist_config(&config)));
            }
            RuntimeEffect::ReloadConfig => {
                execution.completed.push(match ports.load_config() {
                    Ok((config, themes)) => EffectResult::ConfigReloaded {
                        config: Box::new(config),
                        themes,
                    },
                    Err(error) => EffectResult::ConfigReloadFailed(error),
                });
            }
            RuntimeEffect::PersistSessionId(session_id) => {
                ports.persist_session_id(session_id);
            }
            RuntimeEffect::WriteClipboard(text) => {
                let lines = text.lines().count();
                let result = ports.write_clipboard(text);
                execution.completed.push(match result {
                    Ok(()) => EffectResult::ClipboardWritten { lines },
                    Err(error) => EffectResult::ClipboardFailed(error.to_string()),
                });
            }
            RuntimeEffect::RequestDraw(priority) => {
                let reason = match priority {
                    DrawPriority::Interactive => DirtyReason::Interactive,
                    DrawPriority::Content => DirtyReason::Content,
                    DrawPriority::Animation => DirtyReason::Animation,
                };
                scheduler.request(reason, ports.now());
            }
            RuntimeEffect::Quit => execution.quit = true,
            RuntimeEffect::Fatal(reason) => {
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
    let mut config = Config::load();
    // Discover the themes directory (ensuring the two defaults exist) and
    // resolve the configured theme name to a palette. `themes` is refreshed
    // by `/reload` and `/theme`; `config.resolved_theme` caches the result
    // so render-time lookups never touch disk.
    let mut themes = e::theme::load_themes(&Config::themes_dir());
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
    let state = Arc::new(std::sync::Mutex::new(app));
    let mut input = InputState::new(&config);
    let mut scroll = ScrollState::default();
    let mut help_visible = false;
    let mut copy_mode: Option<copy::CopyMode> = None;
    let mut copy_rows_cache = copy::CopyRowsCache::default();
    let mut copy_toast: Option<(String, std::time::Instant)> = None;
    let mut input_page: Option<InputPageSession> = None;
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
    let mut animation_deadline: Option<Instant> = None;
    let mut frame_metrics = FrameMetrics::from_env();
    let mut pending_update_elapsed = Duration::ZERO;
    let mut fatal: Option<String> = None;
    let mut first_draw_done = false;

    'outer: loop {
        let _main_loop_zone = e::tracy_zone!("main loop");
        let mut pending_event = None;
        let mut first_inbound = None;
        let frame_deadline = scheduler.deadline();
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
                let redraw = tick_spinners(&mut state, now);
                if redraw {
                    scheduler.request(DirtyReason::Animation, now);
                }
                animation_deadline = animation_active(&state, now)
                    .then(|| now + animation_interval(&state));
            }
        }

        if let Some(first) = first_inbound {
            let _batch_zone = e::tracy_zone!("inbound batch");
            let batch_started = Instant::now();
            let mut next = Some(first);
            let mut count = 0usize;
            while let Some(msg) = next.take() {
                count += 1;
                let is_snapshot = matches!(&msg, ServerMessage::Snapshot { .. });
                if is_snapshot {
                    phases.mark("snapshot received");
                }
                let update_started = Instant::now();
                let mut ui = BridgeUiState {
                    scroll: &mut scroll,
                    copy_mode: &mut copy_mode,
                    input: &mut input,
                    input_page: &mut input_page,
                };
                let effects = RuntimeController::apply_bridge(msg, &state_r, &mut ui);
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
                    help_visible,
                    input_page_open: input_page.is_some(),
                    approval_open: !drafting && state.approval.is_some(),
                    copy_mode_open: copy_mode.is_some(),
                }
            };
            let route = e::runtime::route_terminal_event(event, focus);
            let terminal_height = terminal.size().map(|size| size.height).unwrap_or(40);
            let effects = RuntimeController::apply_terminal_route(
                route,
                terminal_height,
                runtime_ports.now(),
                &state_r,
                &mut e::runtime::TerminalUiState {
                    scroll: &mut scroll,
                    input: &mut input,
                    input_page: &mut input_page,
                    help_visible: &mut help_visible,
                    copy_mode: &mut copy_mode,
                    copy_rows_cache: &mut copy_rows_cache,
                    copy_toast: &mut copy_toast,
                    config: &mut config,
                    themes: &mut themes,
                    theme: &mut theme,
                },
            );
            let execution =
                execute_runtime_effects(effects, &tx_out, &mut scheduler, &mut runtime_ports).await;
            for result in execution.completed {
                match result {
                    EffectResult::ClipboardWritten { lines } => {
                        copy_toast = Some((format!("已复制 {lines} 行"), runtime_ports.now()));
                    }
                    EffectResult::ConfigReloaded {
                        config: loaded,
                        themes: loaded_themes,
                    } => RuntimeController::apply_reloaded_config(
                        *loaded,
                        loaded_themes,
                        &state_r,
                        &mut e::runtime::TerminalUiState {
                            scroll: &mut scroll,
                            input: &mut input,
                            input_page: &mut input_page,
                            help_visible: &mut help_visible,
                            copy_mode: &mut copy_mode,
                            copy_rows_cache: &mut copy_rows_cache,
                            copy_toast: &mut copy_toast,
                            config: &mut config,
                            themes: &mut themes,
                            theme: &mut theme,
                        },
                    ),
                    other => RuntimeController::apply_effect_result(other, &state_r),
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

        // Start the animation clock only while a running/settling indicator
        // exists. Idle clients have no periodic wakeup.
        if animation_deadline.is_none() {
            let now = Instant::now();
            let state = state_r.lock().unwrap();
            if animation_active(&state, now) {
                animation_deadline = Some(now);
            }
        }

        let now = Instant::now();
        if let Some(requested_at) = scheduler.take_due(now) {
            let mut state = state_r.lock().unwrap();
            // Copy-mode overlay: cursor + selection as global row ranges.
            let overlay = copy_mode.as_ref().and_then(|cm| {
                let rows = copy_rows_cache.rows_with(&state, || e::ui::copy_layout_rows(&state));
                if rows.is_empty() {
                    return None;
                }
                let cursor_row = rows[cm.cursor.min(rows.len() - 1)].global_row;
                let sel = cm
                    .selection_range(rows)
                    .map(|(lo, hi)| (rows[lo].global_row, rows[hi].global_row));
                Some(CopyOverlay { cursor_row, sel })
            });
            let toast = copy_toast.as_ref().map(|(t, _)| t.as_str());
            let first_frame = !first_draw_done;
            let _first_zone = if first_frame {
                e::tracy_zone!("first frame")
            } else {
                None
            };
            let transaction = terminal.draw(|frame| {
                let cursor_anchor = render_with_cursor(
                    frame,
                    &mut state,
                    &input,
                    &mut scroll,
                    &theme,
                    e::ui::RenderOverlays {
                        help_visible,
                        overlay: overlay.as_ref(),
                        toast,
                        input_page: input_page.as_mut(),
                        settings: None,
                        login: None,
                    },
                );
                cursor_anchor
            })?;
            scheduler.complete(Instant::now());
            let io = transaction.io;
            let cache_work = state.transcript_cache.take_work_stats();
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
    fn cli_routes_setup_and_run_without_launcher_for_setup() {
        assert_eq!(
            parse_cli_from(args(&[])).unwrap(),
            CliAction::Run {
                url: "ws://127.0.0.1:3080/dsh-tui".to_string(),
                resume_session_id: None,
            }
        );
        assert_eq!(parse_cli_from(args(&["setup"])).unwrap(), CliAction::Setup);
        assert_eq!(
            parse_cli_from(args(&["ws://host/dsh-tui", "sess-1"])).unwrap(),
            CliAction::Run {
                url: "ws://host/dsh-tui".to_string(),
                resume_session_id: Some("sess-1".to_string()),
            }
        );
    }

    #[test]
    fn cli_rejects_install_and_setup_arguments() {
        let install = parse_cli_from(args(&["install"])).unwrap_err().to_string();
        assert!(install.contains("`dshe setup`"), "{install}");
        assert!(install.contains("install"));

        let extra = parse_cli_from(args(&["setup", "extra"]))
            .unwrap_err()
            .to_string();
        assert!(extra.contains("`dshe setup`"));
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
        state.lock().unwrap().queue.push("next".into());

        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [RuntimeEffect::Send(ClientMessage::Input { text })] if text == "next"
        ));
        let guard = state
            .try_lock()
            .expect("dispatch must not retain the mutex guard");
        assert!(guard.working);
        assert!(guard.queue.is_empty());
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
    fn animation_interval_honors_config_with_safe_floor() {
        let mut state = AppState::default();
        state.config.spinner_frame_ms = 120;
        assert_eq!(animation_interval(&state), Duration::from_millis(120));
        state.config.spinner_frame_ms = 0;
        assert_eq!(animation_interval(&state), MIN_ANIMATION_INTERVAL);
    }
}
