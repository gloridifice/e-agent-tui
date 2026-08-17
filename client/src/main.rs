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
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, MouseEventKind};
use e::config::Config;
use e::copy;
use e::input::{InputAction, InputState, NewMode};
use e::input_page::{InputPageSession, PageEffect};
use e::model::{
    animation_active, tick_spinners, AgentStatus, AppState, ApprovalCard, Msg, QuestionBatch,
};
use e::profile::{FrameMetrics, FrameSample};
use e::protocol::{ClientMessage, ServerMessage, MAX_WIRE_FRAME_BYTES, WIRE_PROTOCOL_VERSION};
use e::terminal_runtime::TerminalOwner;
use e::ui::{
    render_picker, render_with_cursor, scroll_lines, scroll_page, transcript_view_height,
    CopyOverlay, PickerAction, PickerState, ScrollState,
};
use futures_util::StreamExt;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut phases = e::profile::PhaseTimers::new();
    // Tracy: active only with `--features tracy` AND DSH_TUI_TRACY=1.
    #[allow(unused_variables)]
    let _tracy = e::profile::start_tracy();

    let mut args = std::env::args().skip(1);
    let url = args
        .next()
        .unwrap_or_else(|| "ws://127.0.0.1:3080/dsh-tui".to_string());
    let resume_session_id = args.next();

    // Launcher preamble: ensure a DSH bridge is listening at `url`, spawning
    // `dsh --profile dshe` when none is (global dsh, else npx). `dsh_session`
    // records whether this process owns the spawned service so `release`
    // below can shut it down when the last TUI closes.
    let mut dsh_session = e::launcher::acquire(&url, &e::launcher::dsh_home());

    let _z = e::tracy_zone!("read_token");
    let token = read_token()?;
    drop(_z);
    phases.mark("read token");

    let result = run(url, token, resume_session_id, &mut phases).await;

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

/// Mutable locals that bridge messages update — grouped so `handle_msg`
/// keeps a short parameter list instead of five `&mut` tails.
fn is_help_shortcut(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('h')
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
}

pub struct UiChannels<'a> {
    pub picker: &'a mut Option<PickerState>,
    pub scroll: &'a mut ScrollState,
    pub copy_mode: &'a mut Option<copy::CopyMode>,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
}

/// Apply one bridge message to the shared state. Returns a fatal reason when
/// the connection is unusable and the main loop must stop.
fn handle_msg(
    msg: ServerMessage,
    state_r: &Arc<std::sync::Mutex<AppState>>,
    ui: &mut UiChannels<'_>,
) -> Option<String> {
    match &msg {
        ServerMessage::Welcome {
            session_id,
            status,
            provider,
            model,
            mode,
            title,
            cwd,
            ..
        } => {
            let switched = {
                let mut state = state_r.lock().unwrap();
                let switched = state.session_id.as_deref() != Some(session_id.as_str());
                state.apply(
                    "welcome",
                    &serde_json::json!({
                        "sessionId": session_id,
                        "status": status,
                        "provider": provider,
                        "model": model,
                        "mode": mode,
                        "title": title,
                        "cwd": cwd,
                    }),
                );
                switched
            };
            if switched {
                // Fresh transcript (e.g. `/new` or picker attach): the old
                // viewport/copy rows and agent-scoped command directory no
                // longer exist. A fresh `commands` frame follows attach.
                *ui.scroll = ScrollState::default();
                *ui.copy_mode = None;
                ui.input.replace_integrated_commands(Vec::new());
                ui.input.replace_skills(Vec::new());
            }
            // Remember the attached session (D17).
            let mut state_file = e::config::StateFile::load();
            state_file.last_session_id = Some(session_id.clone());
            state_file.save();
            None
        }
        ServerMessage::Snapshot { events, truncated } => {
            let _z = e::tracy_zone!("snapshot apply");
            state_r.lock().unwrap().apply_snapshot(events, *truncated);
            None
        }
        ServerMessage::Event { event } => {
            state_r.lock().unwrap().apply_host_event(event);
            None
        }
        ServerMessage::History { events, has_more } => {
            let events = events.clone();
            let mut state = state_r.lock().unwrap();
            state.prepend_host_events(&events);
            state.history_loading = false;
            state.history_exhausted = !*has_more;
            None
        }
        ServerMessage::Status { status } => {
            state_r
                .lock()
                .unwrap()
                .apply("status", &serde_json::json!({ "status": status }));
            None
        }
        ServerMessage::Sessions { sessions } => {
            let sessions = sessions.clone();
            state_r.lock().unwrap().sessions = sessions.clone();
            if let Some(p) = ui.picker.as_mut() {
                p.sessions = sessions;
            }
            None
        }
        ServerMessage::Presets { presets } => {
            // The `/new <mode>` popup feeds off this roster. Broken presets
            // cannot mount — offering one would invite a failed `/new`; the
            // roster order (declared `order`) is kept as-is.
            ui.input.new_modes.clear();
            ui.input
                .new_modes
                .extend(
                    presets
                        .iter()
                        .filter(|p| p.broken.is_none())
                        .map(|p| NewMode {
                            id: p.id.clone(),
                            name: p.name.clone(),
                            description: p.description.clone(),
                        }),
                );
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_modes(
                    ui.input
                        .new_modes
                        .iter()
                        .map(|mode| mode.id.clone())
                        .collect(),
                );
            }
            None
        }
        ServerMessage::Skills { skills } => {
            ui.input.replace_skills(skills.clone());
            None
        }
        ServerMessage::Title { title } => {
            state_r.lock().unwrap().session_title = Some(title.clone());
            None
        }
        ServerMessage::Commands { commands } => {
            ui.input.replace_integrated_commands(commands.clone());
            None
        }
        ServerMessage::CommandResult {
            command_id,
            kind,
            text,
        } => {
            state_r
                .lock()
                .unwrap()
                .apply_command_result(command_id, kind, text.as_deref());
            None
        }
        ServerMessage::Login {
            providers,
            proxies,
            error,
        } => {
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_login(e::login::LoginView {
                    providers: providers.clone(),
                    proxies: proxies.clone(),
                    error: error.clone(),
                });
            }
            None
        }
        ServerMessage::Model { providers, current } => {
            let cur = current
                .clone()
                .map(|c| (c.provider.clone(), c.model.clone()));
            // Populate the matching open Input Page; late catalog frames are
            // ignored by other pages while global status still updates.
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_model(providers.clone(), cur);
            }
            let mut state = state_r.lock().unwrap();
            if let Some(c) = current {
                state.provider = Some(c.provider.clone());
                state.model = Some(c.model.clone());
            }
            None
        }
        ServerMessage::Approval {
            id,
            tool_name,
            reason,
            call_id,
        } => {
            let _ = call_id;
            state_r.lock().unwrap().approval = Some(ApprovalCard {
                id: id.clone(),
                tool_name: tool_name.clone(),
                reason: reason.clone(),
            });
            None
        }
        ServerMessage::Question {
            rpc_id,
            session_id,
            questions,
        } => {
            state_r.lock().unwrap().question = Some(QuestionBatch::new(
                rpc_id.clone(),
                session_id.clone(),
                questions.clone(),
            ));
            None
        }
        ServerMessage::QuestionResolved {
            question_rpc_id, ..
        } => {
            let mut state = state_r.lock().unwrap();
            if state.question.as_ref().map(|q| q.rpc_id.as_str()) == Some(question_rpc_id.as_str())
            {
                // Settled elsewhere (web GUI, abort…) — drop the selection UI.
                state.question = None;
            }
            None
        }
        ServerMessage::Error { code, message } => {
            if code == "disconnected" {
                Some(format!("bridge disconnected: {message}"))
            } else {
                state_r.lock().unwrap().msgs.push(Msg::Error {
                    text: format!("桥接错误 {code}: {message}"),
                });
                None
            }
        }
        ServerMessage::Pong => None,
    }
}

/// Atomically claim the next queued prompt and mark its turn as started.
///
/// Keeping both mutations under one guard avoids the self-deadlock caused by
/// locking `state` again inside an `if let` whose scrutinee still owns the
/// first `MutexGuard` (Rust 2021 keeps that temporary alive through the body).
fn prepare_next_queued_prompt(state_r: &std::sync::Mutex<AppState>) -> Option<String> {
    let mut state = state_r.lock().unwrap();
    let text = state.take_next_queued()?;
    state.start_thinking();
    Some(text)
}

/// Evaluate a copy-mode key while the state guard is scoped entirely inside
/// this function. The returned action is processed only after the guard drops,
/// so actions such as movement and expand may lock `state` safely again.
fn copy_key_action(
    state_r: &std::sync::Mutex<AppState>,
    copy_mode: &mut copy::CopyMode,
    rows_cache: &mut copy::CopyRowsCache,
    key: &crossterm::event::KeyEvent,
) -> copy::CopyAction {
    let state = state_r.lock().unwrap();
    let rows = rows_cache.rows(&state);
    copy_mode.handle_key(key, rows, &state)
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
    let mut picker: Option<PickerState> = None;
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
    let mut events = EventStream::new();
    let mut scheduler = FrameScheduler::new(Instant::now());
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
            event = events.next() => {
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
                let mut ui = UiChannels {
                    picker: &mut picker,
                    scroll: &mut scroll,
                    copy_mode: &mut copy_mode,
                    input: &mut input,
                    input_page: &mut input_page,
                };
                if let Some(reason) = handle_msg(msg, &state_r, &mut ui) {
                    fatal = Some(reason);
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
        if let Some(text) = prepare_next_queued_prompt(&state_r) {
            let _ = tx_out.send(ClientMessage::Input { text }).await;
            scheduler.request(DirtyReason::Content, Instant::now());
        }

        // ---- directly-woken terminal event ----
        if let Some(ev) = pending_event {
            if let Event::Mouse(mouse) = &ev {
                let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                let down = matches!(mouse.kind, MouseEventKind::ScrollDown);
                if up || down {
                    let terminal_height = terminal.size().map(|size| size.height).unwrap_or(40);
                    let input_page_open = input_page.is_some();
                    let before = {
                        let mut state = state_r.lock().unwrap();
                        let height = transcript_view_height(
                            terminal_height,
                            &state,
                            &input,
                            input_page_open,
                        );
                        scroll_lines(
                            &mut scroll,
                            height,
                            state.transcript_cache.display_len(),
                            up,
                            3,
                        );
                        if up
                            && scroll.offset == 0
                            && !scroll.follow
                            && !state.history_exhausted
                            && !state.history_loading
                        {
                            if let Some(seq) = state.min_seq {
                                state.history_loading = true;
                                Some(seq)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    if let Some(seq) = before {
                        let _ = tx_out
                            .send(ClientMessage::History {
                                before_seq: seq,
                                limit: 400,
                            })
                            .await;
                    }
                }
                continue;
            }
            let Event::Key(key) = ev else {
                // Bracketed paste: route into an Input Page editor, the
                // free-text question draft, or the ordinary input bar.
                if let Event::Paste(text) = ev {
                    if input_page.as_mut().is_some_and(|page| page.paste(&text)) {
                        continue;
                    }
                    {
                        let mut state = state_r.lock().unwrap();
                        if let Some(q) = state.question.as_mut() {
                            if q.is_free_text() {
                                for c in text.chars() {
                                    q.push_char(c);
                                }
                                continue;
                            }
                        }
                    }
                    input.paste(&text);
                }
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            if help_visible {
                if key.code == KeyCode::Char('q')
                    || key.code == KeyCode::Esc
                    || key.code == KeyCode::Char('h')
                {
                    help_visible = false;
                }
                continue;
            }

            // Help is global even while an Input Page owns ordinary input.
            if is_help_shortcut(&key) {
                help_visible = true;
                continue;
            }

            // PageUp/PageDown always operate the transcript, even while an
            // Input Page owns the bottom area.
            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                let up = key.code == KeyCode::PageUp;
                let terminal_height = terminal.size().map(|size| size.height).unwrap_or(40);
                let input_page_open = input_page.is_some();
                let before = {
                    let mut state = state_r.lock().unwrap();
                    let height =
                        transcript_view_height(terminal_height, &state, &input, input_page_open);
                    scroll_page(
                        &mut scroll,
                        height,
                        state.transcript_cache.display_len(),
                        up,
                    );
                    if up
                        && scroll.offset == 0
                        && !scroll.follow
                        && !state.history_exhausted
                        && !state.history_loading
                    {
                        if let Some(seq) = state.min_seq {
                            state.history_loading = true;
                            Some(seq)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(seq) = before {
                    let _ = tx_out
                        .send(ClientMessage::History {
                            before_seq: seq,
                            limit: 400,
                        })
                        .await;
                }
                continue;
            }

            // ---- the active Input Page owns all remaining keys ----
            if input_page.is_some() {
                let outcome = input_page
                    .as_mut()
                    .expect("checked above")
                    .handle_key(&key, &mut config);
                for effect in outcome.effects {
                    match effect {
                        PageEffect::Send(message) => {
                            let _ = tx_out.send(message).await;
                        }
                        PageEffect::ConfigChanged => {
                            config.resolved_theme = e::theme::resolve(&config.theme, &themes);
                            if let Err(error) = config.save() {
                                let mut state = state_r.lock().unwrap();
                                state.msgs.push(Msg::Error {
                                    text: format!("设置保存失败: {error}"),
                                });
                                state.transcript_cache.invalidate();
                            }
                            {
                                let mut state = state_r.lock().unwrap();
                                state.config = config.clone();
                                state.transcript_cache.invalidate();
                            }
                            theme = config.theme();
                            input.paste_placeholder_chars = config.paste_placeholder_chars;
                            input.history_limit = config.history_limit;
                        }
                    }
                }
                if outcome.close {
                    input_page = None;
                }
                continue;
            }

            // ---- session picker owns the keys while open ----
            if let Some(p) = picker.as_mut() {
                match p.handle_key(&key) {
                    PickerAction::None => {}
                    PickerAction::Close => picker = None,
                    PickerAction::Select(id) => {
                        picker = None;
                        let _ = tx_out.send(ClientMessage::Attach { session_id: id }).await;
                    }
                }
                continue;
            }

            // ---- focused blocking input accessory ----
            // Question has higher focus priority than approval; informational
            // accessories never consume keys.
            {
                let pending = {
                    let state = state_r.lock().unwrap();
                    let focused = e::display::focused_blocking_accessory(
                        state.question.is_some(),
                        state.approval.is_some(),
                    );
                    (focused == Some(e::display::InputAccessoryKind::Approval))
                        .then(|| state.approval.clone())
                        .flatten()
                };
                if let Some(card) = pending {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let _ = tx_out
                                .send(ClientMessage::ApprovalAnswer {
                                    id: card.id,
                                    allow: true,
                                })
                                .await;
                            state_r.lock().unwrap().approval = None;
                            continue;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            let _ = tx_out
                                .send(ClientMessage::ApprovalAnswer {
                                    id: card.id,
                                    allow: false,
                                })
                                .await;
                            state_r.lock().unwrap().approval = None;
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            // ---- copy mode owns the keys while active ----
            if let Some(cm) = copy_mode.as_mut() {
                let action = copy_key_action(&state_r, cm, &mut copy_rows_cache, &key);
                match action {
                    copy::CopyAction::None => {}
                    copy::CopyAction::Exit => copy_mode = None,
                    copy::CopyAction::Copy(text) => {
                        let lines_count = text.lines().count();
                        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.clone())) {
                            Ok(()) => {
                                copy_toast = Some((
                                    format!("已复制 {lines_count} 行"),
                                    std::time::Instant::now(),
                                ));
                            }
                            Err(e) => {
                                state_r.lock().unwrap().msgs.push(Msg::Error {
                                    text: format!("剪贴板写入失败: {e}"),
                                });
                            }
                        }
                        copy_mode = None;
                    }
                    copy::CopyAction::ToggleExpand(unit) => {
                        e::presentation::toggle_expand(&mut state_r.lock().unwrap(), unit);
                    }
                    copy::CopyAction::Moved(global_row) => {
                        let height = terminal.size().map(|s| s.height).unwrap_or(40) as usize;
                        let visible = height.saturating_sub(5);
                        let total = state_r.lock().unwrap().msgs.len();
                        let _ = total;
                        scroll.follow = false;
                        let first = scroll.offset;
                        let last = scroll.offset + visible;
                        if global_row < first {
                            scroll.offset = global_row;
                        } else if global_row >= last {
                            scroll.offset = global_row.saturating_sub(visible) + 1;
                        }
                    }
                }
                // Clear the toast when it expires (D19/D28).
                if let Some((_, at)) = &copy_toast {
                    if at.elapsed() > Duration::from_secs(config.copy_toast_secs) {
                        copy_toast = None;
                    }
                }
                continue;
            }

            // ---- user-question mode ----
            let question_action = {
                let mut state = state_r.lock().unwrap();
                e::input::handle_question_key(&mut state, &key)
            };
            if let Some(message) = question_action.outbound {
                let _ = tx_out.send(message).await;
            }
            if question_action.handled {
                continue;
            }

            match key.code {
                KeyCode::Char('n')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    // Open the session picker (design §3.6): fetch the list,
                    // then overlay. Data lands via ServerMessage::Sessions.
                    picker = Some(PickerState::default());
                    let _ = tx_out.send(ClientMessage::ListSessions).await;
                    continue;
                }
                _ => {}
            }
            let idle = state_r.lock().unwrap().status == AgentStatus::Idle;
            let action = input.handle_key(&key, idle);
            match action {
                InputAction::None => {}
                InputAction::Send(text) => {
                    // While the agent runs, the prompt enters the pending
                    // queue (auto-dispatched on idle); otherwise it goes out
                    // immediately with the `• Thinking...` feedback row.
                    let immediate = state_r.lock().unwrap().enqueue_or_immediate(&text);
                    if immediate {
                        state_r.lock().unwrap().start_thinking();
                        let _ = tx_out.send(ClientMessage::Input { text }).await;
                    }
                }
                InputAction::Command(line) => {
                    let outcome = e::runtime_command::handle_local_command(
                        line,
                        e::runtime_command::LocalCommandContext {
                            input_page: &mut input_page,
                            picker: &mut picker,
                            help_visible: &mut help_visible,
                            copy_mode: &mut copy_mode,
                            config: &mut config,
                            themes: &mut themes,
                            input: &mut input,
                            theme: &mut theme,
                            state: &state_r,
                            outbound: &tx_out,
                        },
                    )
                    .await;
                    if matches!(outcome, e::runtime_command::CommandOutcome::Quit) {
                        break 'outer;
                    }
                }
                InputAction::Interrupt => {
                    // Esc stops the whole plan: drop prompts that never left
                    // the client queue.
                    state_r.lock().unwrap().queue.clear();
                    let _ = tx_out.send(ClientMessage::Interrupt).await;
                }
                InputAction::Quit => {
                    break 'outer;
                }
                InputAction::CopyMode => {
                    // Enter copy mode over the assistant transcript (D12).
                    copy_mode = Some(copy::CopyMode::default());
                }
                InputAction::ToggleMultiline => {}
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
                let rows = copy_rows_cache.rows(&state);
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
                let mut cursor_anchor = render_with_cursor(
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
                if let Some(p) = picker.as_ref() {
                    render_picker(frame, p, &theme);
                    cursor_anchor = None;
                }
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
    terminal.restore().ok();
    if let Some(reason) = fatal {
        bail!("{reason}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    #[test]
    fn help_shortcut_is_global_input() {
        assert!(is_help_shortcut(&KeyEvent::new(
            KeyCode::Char('h'),
            KeyModifiers::CONTROL,
        )));
        assert!(!is_help_shortcut(&KeyEvent::new(
            KeyCode::Char('h'),
            KeyModifiers::NONE,
        )));
    }

    #[test]
    fn shutdown_confirmation_has_the_required_text() {
        assert_eq!(DSH_SERVER_CLOSED_MESSAGE, "dsh 服务器已关闭。");
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

        assert_eq!(prepare_next_queued_prompt(&state).as_deref(), Some("next"));
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

    #[test]
    fn copy_key_evaluation_releases_the_state_lock() {
        let mut app = AppState::default();
        app.msgs.push(Msg::Assistant {
            text: "a\nb".into(),
            lines: vec![
                e::render::RenderLine {
                    line: ratatui::text::Line::from("a"),
                    unit: 1,
                    raw_line: Some(0),
                    atomic: false,
                    fill: false,
                },
                e::render::RenderLine {
                    line: ratatui::text::Line::from("b"),
                    unit: 1,
                    raw_line: Some(1),
                    atomic: false,
                    fill: false,
                },
            ],
            unit_start: 1,
        });
        app.transcript_cache.width = 80;
        let state = std::sync::Mutex::new(app);
        let mut rows_cache = copy::CopyRowsCache::default();
        let mut copy_mode = copy::CopyMode::default();
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);

        let action = copy_key_action(&state, &mut copy_mode, &mut rows_cache, &key);
        assert!(matches!(action, copy::CopyAction::Moved(1)));
        assert!(
            state.try_lock().is_ok(),
            "copy action must not retain the mutex guard"
        );
    }
}
