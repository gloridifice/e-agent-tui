//! e — terminal client for DeepSeek Harness (the `dshe` executable).
//!
//! One tokio runtime: a websocket reader forwards bridge messages, a writer
//! drains outbound commands, and the main loop polls crossterm keys, applies
//! inbound state, and repaints the ratatui frame on a 50 ms tick.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use e::config::Config;
use e::copy;
use e::input::{InputAction, InputState, NewMode};
use e::login::{LoginAction, LoginState};
use e::model::{tick_spinners, AgentStatus, AppState, ApprovalCard, Msg, QuestionBatch};
use e::protocol::{ClientMessage, ServerMessage};
use e::settings;
use e::ui::{render, render_picker, scroll_page, CopyOverlay, PickerState, ScrollState};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::Message;

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
    // `dsh --profile tui` when none is (global dsh, else npx). `dsh_session`
    // records whether this process owns the spawned service so `release`
    // below can shut it down when the last TUI closes.
    let mut dsh_session = e::launcher::acquire(&url, &e::launcher::dsh_home());

    let _z = e::tracy_zone!("read_token");
    let token = read_token()?;
    drop(_z);
    phases.mark("read token");

    // ---- terminal setup ----
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        crossterm::event::EnableBracketedPaste,
        EnterAlternateScreen,
        cursor::Hide
    )
    .context("enter alternate screen")?;
    phases.mark("terminal setup");

    let result = run(url, token, resume_session_id, &mut phases).await;

    // On TUI exit: release the launcher bookkeeping. A dshe-spawned service
    // is shut down when this is the last attached TUI; an out-of-band dsh is
    // left untouched.
    e::launcher::release(&mut dsh_session);

    disable_raw_mode().ok();
    execute!(
        stdout,
        LeaveAlternateScreen,
        cursor::Show,
        crossterm::event::DisableBracketedPaste
    )
    .ok();
    result
}

/// Mutable locals that bridge messages update — grouped so `handle_msg`
/// keeps a short parameter list instead of five `&mut` tails.
pub struct UiChannels<'a> {
    pub picker: &'a mut Option<PickerState>,
    pub scroll: &'a mut ScrollState,
    pub copy_mode: &'a mut Option<copy::CopyMode>,
    pub new_modes: &'a mut Vec<NewMode>,
    pub login: &'a mut Option<LoginState>,
    pub model_picker: &'a mut Option<e::ui::ModelPicker>,
}

/// Apply one bridge message to the shared state. Returns a fatal reason when
/// the connection is unusable and the main loop must stop.
fn handle_msg(
    msg: ServerMessage,
    state_r: &Arc<std::sync::Mutex<AppState>>,
    ui: &mut UiChannels<'_>,
) -> Option<String> {
    match &msg {
        ServerMessage::Welcome { session_id, status, provider, model, title, cwd } => {
            let switched = {
                let mut state = state_r.lock().unwrap();
                let switched = state.session_id.as_deref() != Some(session_id.as_str());
                state.apply("welcome", &serde_json::json!({
                    "sessionId": session_id,
                    "status": status,
                    "provider": provider,
                    "model": model,
                    "title": title,
                    "cwd": cwd,
                }));
                switched
            };
            if switched {
                // Fresh transcript (e.g. `/new` or picker attach): the old
                // viewport and copy-mode rows no longer exist.
                *ui.scroll = ScrollState::default();
                *ui.copy_mode = None;
            }
            // Remember the attached session (D17).
            let mut state_file = e::config::StateFile::load();
            state_file.last_session_id = Some(session_id.clone());
            state_file.save();
            None
        }
        ServerMessage::Snapshot { events, truncated } => {
            let _z = e::tracy_zone!("snapshot apply");
            state_r.lock().unwrap().apply(
                "snapshot",
                &serde_json::json!({ "events": events, "truncated": truncated }),
            );
            None
        }
        ServerMessage::Event { event } => {
            state_r.lock().unwrap().apply_event(event);
            None
        }
        ServerMessage::History { events, has_more } => {
            let events = events.clone();
            let mut state = state_r.lock().unwrap();
            state.prepend_events(&events);
            state.history_loading = false;
            state.history_exhausted = !*has_more;
            None
        }
        ServerMessage::Status { status } => {
            state_r.lock().unwrap().apply("status", &serde_json::json!({ "status": status }));
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
            ui.new_modes.clear();
            ui.new_modes.extend(
                presets
                    .iter()
                    .filter(|p| p.broken.is_none())
                    .map(|p| NewMode {
                        id: p.id.clone(),
                        name: p.name.clone(),
                        description: p.description.clone(),
                    }),
            );
            None
        }
        ServerMessage::Title { title } => {
            state_r.lock().unwrap().session_title = Some(title.clone());
            None
        }
        ServerMessage::Login { providers, proxies, codex, error } => {
            if let Some(l) = ui.login.as_mut() {
                l.apply(e::login::LoginView {
                    providers: providers.clone(),
                    proxies: proxies.clone(),
                    codex: codex.clone(),
                    error: error.clone(),
                });
            }
            None
        }
        ServerMessage::LoginCodex { status, user_code, verification_uri, account_id, error } => {
            if let Some(l) = ui.login.as_mut() {
                l.apply_codex(e::login::CodexView {
                    status: status.clone(),
                    user_code: user_code.clone(),
                    verification_uri: verification_uri.clone(),
                    account_id: account_id.clone(),
                    error: error.clone(),
                });
            }
            None
        }
        ServerMessage::Model { providers, current } => {
            let cur = current.clone().map(|c| (c.provider.clone(), c.model.clone()));
            // Populate the open picker (if any). The frame also arrives after
            // `model-set` — by then the picker is closed, so only the state
            // below is updated.
            if let Some(mp) = ui.model_picker.as_mut() {
                *mp = e::ui::ModelPicker::new(providers.clone(), cur);
            }
            let mut state = state_r.lock().unwrap();
            if let Some(c) = current {
                state.provider = Some(c.provider.clone());
                state.model = Some(c.model.clone());
            }
            None
        }
        ServerMessage::Approval { id, tool_name, reason, call_id } => {
            let _ = call_id;
            state_r.lock().unwrap().approval = Some(ApprovalCard {
                id: id.clone(),
                tool_name: tool_name.clone(),
                reason: reason.clone(),
            });
            None
        }
        ServerMessage::Question { rpc_id, session_id, questions } => {
            state_r.lock().unwrap().question = Some(QuestionBatch::new(
                rpc_id.clone(),
                session_id.clone(),
                questions.clone(),
            ));
            None
        }
        ServerMessage::QuestionResolved { question_rpc_id, .. } => {
            let mut state = state_r.lock().unwrap();
            if state
                .question
                .as_ref()
                .map(|q| q.rpc_id.as_str())
                == Some(question_rpc_id.as_str())
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
    let mut copy_toast: Option<(String, std::time::Instant)> = None;
    let mut picker: Option<PickerState> = None;
    let mut settings: Option<settings::SettingsState> = None;
    let mut login: Option<LoginState> = None;
    let mut theme_picker: Option<e::ui::ThemePicker> = None;
    let mut model_picker: Option<e::ui::ModelPicker> = None;
    let mut theme = theme;

    let (ws, _) = {
        // Huge session logs: the snapshot frame can exceed tungstenite's
        // default message cap (bridge-side trimming lands with the next DSH
        // restart; this keeps large replays working until then).
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
            max_message_size: Some(512 * 1024 * 1024),
            max_frame_size: Some(512 * 1024 * 1024),
            ..Default::default()
        };
        let _z = e::tracy_zone!("ws connect");
        let ws = connect_async_with_config(&url, Some(config), false)
            .await
            .with_context(|| format!("connect {url}"))?;
        drop(_z);
        ws
    };
    phases.mark("ws connect");
    let (sink, mut stream) = ws.split();

    // ---- outbound queue: ui keys / stdin-independent ----
    let (tx_out, mut rx_out) = mpsc::channel::<ClientMessage>(128);
    // The launch directory is "the current directory" for new sessions:
    // the bridge opens the fresh session's workspace there (the attached
    // session's header cwd is only the fallback). `mode` names the
    // configured default preset for the startup session — sent only when
    // creating one.
    let launch_cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    tx_out
        .send(ClientMessage::Hello {
            token,
            resume_session_id: resume_session_id.clone(),
            cwd: launch_cwd,
            mode: if resume_session_id.is_none() {
                Some(config.default_mode.clone())
            } else {
                None
            },
        })
        .await?;
    phases.mark("hello sent");
    let writer = tokio::spawn(async move {
        let mut sink = sink;
        while let Some(msg) = rx_out.recv().await {
            if sink.send(Message::Text(msg.to_wire().unwrap())).await.is_err() {
                break;
            }
        }
    });

    // ---- inbound queue: bridge messages -> main loop ----
    let (tx_in, mut rx_in) = mpsc::channel::<ServerMessage>(512);
    let state_r = Arc::clone(&state);
    let reader = tokio::spawn(async move {
        while let Some(item) = stream.next().await {
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    eprintln!("[dshe] websocket stream error: {error}");
                    break;
                }
            };
            match item {
                Message::Text(text) => {
                    if let Some(msg) = ServerMessage::from_wire(&text) {
                        if tx_in.send(msg).await.is_err() {
                            break;
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        // Signal disconnection through an error message.
        let _ = tx_in.send(ServerMessage::Error {
            code: "disconnected".into(),
            message: "bridge connection closed".into(),
        }).await;
    });

    // ---- main loop ----
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut terminal = ratatui::init();
    let mut fatal: Option<String> = None;
    // Redraw throttle: at most one frame per 30 ms, and only when something
    // changed (events, keys, spinner frames). Streaming chunks arrive at
    // high frequency; drawing per chunk made output crawl.
    let mut dirty = true;
    let mut last_render: Option<std::time::Instant> = None;
    let mut first_draw_done = false;

    'outer: loop {
        tokio::select! {
            _ = tick.tick() => {}
            maybe = rx_in.recv() => {
                let Some(msg) = maybe else {
                    fatal = Some("bridge disconnected".into());
                    break 'outer;
                };
                let is_snapshot = matches!(&msg, ServerMessage::Snapshot { .. });
                if is_snapshot {
                    phases.mark("snapshot received");
                }
                let mut ui = UiChannels {
                    picker: &mut picker,
                    scroll: &mut scroll,
                    copy_mode: &mut copy_mode,
                    new_modes: &mut input.new_modes,
                    login: &mut login,
                    model_picker: &mut model_picker,
                };
                if let Some(reason) = handle_msg(msg, &state_r, &mut ui) {
                    fatal = Some(reason);
                    break 'outer;
                }
                if is_snapshot {
                    phases.mark("snapshot applied");
                }
                dirty = true;
            }
        }

        // Drain any backlog so event bursts coalesce into a single redraw
        // instead of one full frame per chunk.
        loop {
            match rx_in.try_recv() {
                Ok(msg) => {
                    let mut ui = UiChannels {
                        picker: &mut picker,
                        scroll: &mut scroll,
                        copy_mode: &mut copy_mode,
                        new_modes: &mut input.new_modes,
                        login: &mut login,
                        model_picker: &mut model_picker,
                    };
                    if let Some(reason) = handle_msg(msg, &state_r, &mut ui) {
                        fatal = Some(reason);
                        break 'outer;
                    }
                    dirty = true;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    fatal = Some("bridge disconnected".into());
                    break 'outer;
                }
            }
        }

        // ---- queued prompts: auto-dispatch the next one now that the agent
        // ---- is idle (one at a time — each dispatch keeps it busy again).
        if let Some(text) = state_r.lock().unwrap().take_next_queued() {
            state_r.lock().unwrap().start_thinking();
            let _ = tx_out.send(ClientMessage::Input { text }).await;
            dirty = true;
        }

        // ---- key events ----
        while event::poll(Duration::ZERO).unwrap_or(false) {
            let Ok(ev) = event::read() else { continue };
            let Event::Key(key) = ev else {
                // Bracketed paste: route into the settings edit buffer, the
                // free-text question draft, or the input bar. Pastes over
                // the threshold become an atomic paste block (input.rs).
                if let Event::Paste(text) = ev {
                    dirty = true;
                    if let Some(s) = settings.as_mut() {
                        if let Some(settings::Edit::Input { buf }) = &mut s.editing {
                            buf.push_str(&text);
                            continue;
                        }
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
            dirty = true;
            if help_visible {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc || key.code == KeyCode::Char('h') {
                    help_visible = false;
                }
                continue;
            }

            // ---- /settings overlay owns the keys while open ----
            if let Some(s) = settings.as_mut() {
                match s.handle_key(&key, &mut config) {
                    settings::SettingsAction::None => {}
                    settings::SettingsAction::Exit => settings = None,
                    settings::SettingsAction::Changed => {
                        // The theme name may have changed via /settings —
                        // resolve it against the theme registry before saving.
                        config.resolved_theme = e::theme::resolve(&config.theme, &themes);
                        if let Err(error) = config.save() {
                            state_r.lock().unwrap().msgs.push(Msg::Error {
                                text: format!("设置保存失败: {error}"),
                            });
                        }
                        {
                            let mut state = state_r.lock().unwrap();
                            state.config = config.clone();
                            // Padding/theme changes are baked into the cached
                            // lines — rebuild on the next draw.
                            state.cache_valid = false;
                        }
                        theme = config.theme();
                        input.paste_placeholder_chars = config.paste_placeholder_chars;
                        input.enter_sends = config.enter_sends;
                        input.history_limit = config.history_limit;
                    }
                }
                continue;
            }

            // ---- /login panel owns the keys while open ----
            if let Some(l) = login.as_mut() {
                match l.handle_key(&key) {
                    LoginAction::None => {}
                    LoginAction::Exit => login = None,
                    LoginAction::Send(msg) => {
                        // Every confirmed action goes to the bridge; the
                        // refreshed `login`/`login-codex` frame updates the panel.
                        let _ = tx_out.send(msg).await;
                    }
                }
                continue;
            }

            // ---- session picker owns the keys while open ----
            if let Some(p) = picker.as_mut() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => picker = None,
                    KeyCode::Up => {
                        p.sel = p.sel.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let n = p.filtered().len();
                        if n > 0 {
                            p.sel = (p.sel + 1).min(n - 1);
                        }
                    }
                    KeyCode::Enter => {
                        let filtered = p.filtered();
                        if let Some(&idx) = filtered.get(p.sel) {
                            let id = p.sessions[idx].id.clone();
                            picker = None;
                            let _ = tx_out.send(ClientMessage::Attach { session_id: id }).await;
                        }
                    }
                    KeyCode::Char(c) => {
                        if c == ' ' || (!c.is_ascii_control()) {
                            p.query.push(c);
                            p.sel = 0;
                        }
                    }
                    KeyCode::Backspace => {
                        p.query.pop();
                        p.sel = 0;
                    }
                    _ => {}
                }
                continue;
            }

            // ---- /theme picker owns the keys while open ----
            if let Some(tp) = theme_picker.as_mut() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => theme_picker = None,
                    KeyCode::Up | KeyCode::Char('k') => {
                        tp.sel = tp.sel.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let n = tp.themes.len();
                        if n > 0 {
                            tp.sel = (tp.sel + 1).min(n - 1);
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(name) = tp.themes.get(tp.sel).map(|t| t.name.clone()) {
                            // Apply: persist the name, resolve the palette,
                            // and rebuild the cached transcript with it.
                            config.theme = name;
                            config.resolved_theme = e::theme::resolve(&config.theme, &themes);
                            if let Err(error) = config.save() {
                                state_r.lock().unwrap().msgs.push(Msg::Error {
                                    text: format!("主题保存失败: {error}"),
                                });
                            }
                            {
                                let mut state = state_r.lock().unwrap();
                                state.config = config.clone();
                                state.cache_valid = false;
                            }
                            theme = config.theme();
                            theme_picker = None;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // ---- /model picker owns the keys while open ----
            if let Some(mp) = model_picker.as_mut() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => model_picker = None,
                    KeyCode::Left | KeyCode::Char('h') => {
                        if mp.prov_sel > 0 {
                            mp.prov_sel -= 1;
                            mp.model_sel = 0;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if mp.prov_sel + 1 < mp.providers.len() {
                            mp.prov_sel += 1;
                            mp.model_sel = 0;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        mp.model_sel = mp.model_sel.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let n = mp.providers.get(mp.prov_sel).map(|p| p.models.len()).unwrap_or(0);
                        if n > 0 {
                            mp.model_sel = (mp.model_sel + 1).min(n - 1);
                        }
                    }
                    KeyCode::Enter => {
                        if let Some((provider, model)) = mp.selected() {
                            model_picker = None;
                            let _ = tx_out.send(ClientMessage::ModelSet { provider, model }).await;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // ---- approval answer keys (design §4.4) ----
            {
                let pending = state_r.lock().unwrap().approval.clone();
                if let Some(card) = pending {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let _ = tx_out
                                .send(ClientMessage::ApprovalAnswer { id: card.id, allow: true })
                                .await;
                            state_r.lock().unwrap().approval = None;
                            continue;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            let _ = tx_out
                                .send(ClientMessage::ApprovalAnswer { id: card.id, allow: false })
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
                let rows = copy::flatten(&state_r.lock().unwrap());
                match cm.handle_key(&key, &rows, &state_r.lock().unwrap()) {
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
                        state_r.lock().unwrap().toggle_expand(unit);
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

            // ---- user-question mode: the selection bar owns ←→/Enter/Esc ----
            // (design §4.4: Enter picks the highlighted option and moves to
            // the next question; on the last question Enter confirms. Esc
            // cancels the whole batch. Free-text questions collect typed
            // characters instead.)
            {
                let mut out: Option<ClientMessage> = None;
                let mut handled = false;
                {
                    let mut state = state_r.lock().unwrap();
                    if let Some(q) = state.question.as_mut() {
                        match key.code {
                            KeyCode::Left => {
                                q.step(-1);
                                handled = true;
                            }
                            KeyCode::Right => {
                                q.step(1);
                                handled = true;
                            }
                            KeyCode::Enter => {
                                if let Some(answers) = q.enter() {
                                    out = Some(ClientMessage::AnswerQuestions {
                                        rpc_id: q.rpc_id.clone(),
                                        answers,
                                    });
                                    state.question = None;
                                }
                                handled = true;
                            }
                            KeyCode::Esc => {
                                out = Some(ClientMessage::CancelQuestions {
                                    rpc_id: q.rpc_id.clone(),
                                });
                                state.question = None;
                                handled = true;
                            }
                            KeyCode::Backspace => {
                                q.backspace();
                                handled = true;
                            }
                            KeyCode::Char(c)
                                if key.modifiers.is_empty()
                                    || key.modifiers
                                        == crossterm::event::KeyModifiers::SHIFT =>
                            {
                                if c == ' ' || !c.is_ascii_control() {
                                    q.push_char(c);
                                }
                                handled = true;
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(msg) = out {
                    let _ = tx_out.send(msg).await;
                }
                if handled {
                    continue;
                }
            }

            match key.code {
                KeyCode::PageUp => {
                    let height = terminal.size().map(|s| s.height).unwrap_or(40) as usize;
                    // Lazy scroll-back: at the very top with older history
                    // still available, request the previous page.
                    let before = {
                        let mut state = state_r.lock().unwrap();
                        scroll_page(&mut scroll, height, state.render_cache.len(), true);
                        if scroll.offset == 0
                            && !scroll.follow
                            && !state.history_exhausted
                            && !state.history_loading
                        {
                            state.min_seq.map(|seq| {
                                state.history_loading = true;
                                seq
                            })
                        } else {
                            None
                        }
                    };
                    if let Some(seq) = before {
                        let _ = tx_out
                            .send(ClientMessage::History { before_seq: seq, limit: 400 })
                            .await;
                    }
                    continue;
                }
                KeyCode::PageDown => {
                    let height = terminal.size().map(|s| s.height).unwrap_or(40) as usize;
                    let total = state_r.lock().unwrap().render_cache.len();
                    scroll_page(&mut scroll, height, total, false);
                    continue;
                }
                KeyCode::Char('h') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                    help_visible = true;
                    continue;
                }
                KeyCode::Char('n') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
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
                    // Local commands never leave the client.
                    if line == "/settings" {
                        let mut s = settings::SettingsState::default();
                        // 默认模式 choice feeds off the live roster.
                        s.modes = input.new_modes.iter().map(|m| m.id.clone()).collect();
                        // 主题 choice feeds off the discovered theme files.
                        s.themes = themes.iter().map(|t| t.name.clone()).collect();
                        settings = Some(s);
                    } else if line == "/login" {
                        // The input bar becomes the login settings page;
                        // its state comes from the bridge (`login` frame).
                        login = Some(LoginState::default());
                        let _ = tx_out.send(ClientMessage::LoginGet).await;
                    } else if line == "/theme" {
                        // Theme selector: discovered theme files with a
                        // color swatch; Enter applies and persists the name.
                        theme_picker = Some(e::ui::ThemePicker::from_themes(&themes, &config.theme));
                    } else if line == "/model" {
                        // Model selector: providers × models fed by the
                        // bridge's `model` frame; Enter applies to the session.
                        model_picker = Some(e::ui::ModelPicker::new(Vec::new(), None));
                        let _ = tx_out.send(ClientMessage::ModelGet).await;
                    } else if line == "/reload" {
                        // Re-read config + rescan the theme registry (and
                        // later skills); keep the current session attached.
                        config = Config::load();
                        themes = e::theme::load_themes(&Config::themes_dir());
                        config.resolved_theme = e::theme::resolve(&config.theme, &themes);
                        {
                            let mut state = state_r.lock().unwrap();
                            state.config = config.clone();
                            state.cache_valid = false;
                        }
                        theme = config.theme();
                        input.paste_placeholder_chars = config.paste_placeholder_chars;
                        input.enter_sends = config.enter_sends;
                        input.history_limit = config.history_limit;
                        state_r.lock().unwrap().msgs.push(Msg::System {
                            text: "已重载配置、主题与技能".into(),
                        });
                    } else if line == "/help" {
                        help_visible = true;
                    } else if line == "/copy" {
                        copy_mode = Some(copy::CopyMode::default());
                    } else if line == "/exit" || line == "/q" || line == "/quit" {
                        // Quit the TUI only — the conversation keeps running
                        // in DSH (the agent is not interrupted).
                        break 'outer;
                    } else if line == "/resume" {
                        // Open the session picker (same overlay as Ctrl+N).
                        picker = Some(PickerState::default());
                        let _ = tx_out.send(ClientMessage::ListSessions).await;
                    } else if let Some(id) = line.strip_prefix("/resume ") {
                        let id = id.trim();
                        if !id.is_empty() {
                            let _ = tx_out
                                .send(ClientMessage::Attach { session_id: id.to_string() })
                                .await;
                        }
                    } else {
                        state_r.lock().unwrap().start_thinking();
                        let _ = tx_out.send(ClientMessage::Command { line }).await;
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

        // ---- render (throttled: ≤1 frame/30 ms, only when dirty) ----
        let now = std::time::Instant::now();
        {
            let mut state = state_r.lock().unwrap();
            if tick_spinners(&mut state, now) {
                dirty = true;
            }
        }
        let draw_due = last_render.map_or(true, |t| {
            now.duration_since(t) >= Duration::from_millis(30)
        });
        if dirty && draw_due {
            {
                let mut state = state_r.lock().unwrap();
                // Copy-mode overlay: cursor + selection as global row ranges.
                let overlay = copy_mode.as_ref().and_then(|cm| {
                    let rows = copy::flatten(&state);
                    if rows.is_empty() {
                        return None;
                    }
                    let cursor_row = rows[cm.cursor.min(rows.len() - 1)].global_row;
                    let sel = cm.selection_range(&rows).map(|(lo, hi)| {
                        (rows[lo].global_row, rows[hi].global_row)
                    });
                    Some(CopyOverlay { cursor_row, sel })
                });
                let toast = copy_toast.as_ref().map(|(t, _)| t.as_str());
                let first_frame = !first_draw_done;
                let _z = if first_frame {
                    e::tracy_zone!("first frame")
                } else {
                    None
                };
                terminal.draw(|frame| {
                    render(frame, &mut state, &input, &mut scroll, &theme, e::ui::RenderOverlays {
                        help_visible,
                        overlay: overlay.as_ref(),
                        toast,
                        settings: settings.as_mut(),
                        login: login.as_mut(),
                    });
                    if let Some(p) = picker.as_ref() {
                        render_picker(frame, p, &theme);
                    }
                    if let Some(tp) = theme_picker.as_ref() {
                        e::ui::render_theme_picker(frame, tp, &theme);
                    }
                    if let Some(mp) = model_picker.as_ref() {
                        e::ui::render_model_picker(frame, mp, &theme);
                    }
                })?;
                if first_frame {
                    drop(_z);
                    first_draw_done = true;
                    phases.mark("first frame");
                }
            }
            last_render = Some(now);
            dirty = false;
        }
    }

    writer.abort();
    reader.abort();
    ratatui::restore();
    if let Some(reason) = fatal {
        bail!("{reason}");
    }
    Ok(())
}
