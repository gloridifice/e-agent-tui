//! Testable runtime inputs, effects, and bridge-message controller.
//!
//! The Tokio loop in `main.rs` owns waiting and concrete I/O. This module
//! mutates typed client state and returns effects that the runner executes only
//! after state guards have been released.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};

#[cfg(test)]
use crate::model::Msg;
use crate::{
    command_catalog::NewMode,
    config::Config,
    copy,
    input::{InputAction, InputState},
    input_page::{InputPageSession, PageEffect},
    login::LoginView,
    model::{AppState, ApprovalCard, QuestionBatch},
    protocol::{ClientMessage, ServerMessage, WIRE_PROTOCOL_VERSION},
    runtime_command::{self, LocalCommandContext},
    theme::{self, Theme, ThemeFile},
    ui::{scroll_lines, scroll_page, transcript_view_height, ScrollState},
};

pub enum RuntimeInput {
    Bridge(ServerMessage),
    Terminal(Event),
    EffectCompleted(EffectResult),
    AnimationDeadline,
    FrameDeadline,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalFocus {
    pub help_visible: bool,
    pub input_page_open: bool,
    pub question_open: bool,
    pub approval_open: bool,
    pub copy_mode_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalRoute {
    MouseScroll { up: bool },
    Paste { text: String },
    Help { dismiss: bool },
    OpenHelp,
    TranscriptPage { up: bool },
    InputPage(KeyEvent),
    Approval(KeyEvent),
    Copy(KeyEvent),
    QuestionThenOrdinary(KeyEvent),
    Ordinary(KeyEvent),
    Ignore,
}

/// Encode terminal ownership and precedence independently of terminal I/O.
/// Handlers may return to the ordinary-input path only where the legacy
/// behavior intentionally allowed fall-through (approval non-y/n and an
/// unhandled question key).
pub fn route_terminal_event(event: Event, focus: TerminalFocus) -> TerminalRoute {
    match event {
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => TerminalRoute::MouseScroll { up: true },
            MouseEventKind::ScrollDown => TerminalRoute::MouseScroll { up: false },
            _ => TerminalRoute::Ignore,
        },
        Event::Paste(text) => TerminalRoute::Paste { text },
        Event::Key(key) if key.kind == KeyEventKind::Release => TerminalRoute::Ignore,
        Event::Key(key) if focus.help_visible => TerminalRoute::Help {
            dismiss: matches!(
                key.code,
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h')
            ),
        },
        Event::Key(key)
            if key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            TerminalRoute::OpenHelp
        }
        Event::Key(key) if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) => {
            TerminalRoute::TranscriptPage {
                up: key.code == KeyCode::PageUp,
            }
        }
        Event::Key(key) if focus.input_page_open => TerminalRoute::InputPage(key),
        Event::Key(key)
            if focus.approval_open
                && !focus.question_open
                && matches!(key.code, KeyCode::Char('y' | 'Y' | 'n' | 'N')) =>
        {
            TerminalRoute::Approval(key)
        }
        Event::Key(key) if focus.copy_mode_open => TerminalRoute::Copy(key),
        Event::Key(key) if focus.question_open => TerminalRoute::QuestionThenOrdinary(key),
        Event::Key(key) => TerminalRoute::Ordinary(key),
        _ => TerminalRoute::Ignore,
    }
}

#[derive(Debug)]
pub enum EffectResult {
    ConfigPersisted(Result<(), String>),
    ConfigReloaded {
        config: Box<Config>,
        themes: Vec<ThemeFile>,
    },
    ConfigReloadFailed(String),
    ClipboardWritten {
        lines: usize,
    },
    ClipboardFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawPriority {
    Interactive,
    Content,
    Animation,
}

/// Pure state transitions produced by handlers and consumed by the controller
/// while it owns the relevant UI state. These never reach infrastructure.
pub enum ControllerAction {
    OpenPage(InputPageSession),
    ClosePage,
}

/// Work that must execute after all controller state guards have been dropped.
/// Every variant owns the complete payload required by its infrastructure port.
pub enum RuntimeEffect {
    Send(ClientMessage),
    PersistConfig(Config),
    ReloadConfig,
    PersistSessionId(String),
    WriteClipboard(String),
    RequestDraw(DrawPriority),
    Quit,
    Fatal(String),
}

/// Mutable UI-local state affected by bridge frames. Keeping it separate from
/// AppState makes session-switch behavior explicit without giving the bridge
/// handler ownership of terminal or transport infrastructure.
pub struct BridgeUiState<'a> {
    pub scroll: &'a mut ScrollState,
    pub copy_mode: &'a mut Option<copy::CopyMode>,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
}

#[derive(Default)]
pub struct InputHandlerOutcome {
    pub command: Option<String>,
    pub activate_copy_mode: bool,
    pub effects: Vec<RuntimeEffect>,
}

pub struct InputPageUiState<'a> {
    pub input_page: &'a mut Option<InputPageSession>,
    pub input: &'a mut InputState,
    pub config: &'a mut Config,
    pub themes: &'a [ThemeFile],
    pub theme: &'a mut Theme,
}

pub struct TerminalUiState<'a> {
    pub scroll: &'a mut ScrollState,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
    pub help_visible: &'a mut bool,
    pub copy_mode: &'a mut Option<copy::CopyMode>,
    pub copy_rows_cache: &'a mut copy::CopyRowsCache,
    pub copy_toast: &'a mut Option<(String, Instant)>,
    pub config: &'a mut Config,
    pub themes: &'a mut Vec<ThemeFile>,
    pub theme: &'a mut Theme,
}

pub struct RuntimeController;

fn protocol_mismatch_fatal(detail: &str) -> String {
    format!(
        "bridge protocol mismatch: {detail}. Update the client and bridge from the same checkout: remount with `tools\\mount-bridge.ps1 -Profile dshe`, run `dsh plugin --profile dshe install`, rebuild/reinstall `dshe`, and restart DSH"
    )
}

impl RuntimeController {
    pub fn apply_terminal_route(
        route: TerminalRoute,
        terminal_height: u16,
        now: Instant,
        state: &Arc<Mutex<AppState>>,
        ui: &mut TerminalUiState<'_>,
    ) -> Vec<RuntimeEffect> {
        let mut effects = Vec::new();
        match route {
            route @ (TerminalRoute::MouseScroll { up } | TerminalRoute::TranscriptPage { up }) => {
                let page = matches!(route, TerminalRoute::TranscriptPage { .. });
                let before = {
                    let mut app = state.lock().unwrap();
                    let height = transcript_view_height(
                        terminal_height,
                        &app,
                        ui.input,
                        ui.input_page.is_some(),
                    );
                    if page {
                        scroll_page(ui.scroll, height, app.transcript_cache.display_len(), up);
                    } else {
                        scroll_lines(ui.scroll, height, app.transcript_cache.display_len(), up, 3);
                    }
                    if up
                        && ui.scroll.offset == 0
                        && !ui.scroll.follow
                        && !app.history_exhausted
                        && !app.history_loading
                    {
                        app.min_seq.map(|seq| {
                            app.history_loading = true;
                            seq
                        })
                    } else {
                        None
                    }
                };
                if let Some(before_seq) = before {
                    effects.push(RuntimeEffect::Send(ClientMessage::History {
                        before_seq,
                        limit: 400,
                    }));
                }
            }
            TerminalRoute::Paste { text } => {
                if ui.input_page.as_mut().is_some_and(|page| page.paste(&text)) {
                    return effects;
                }
                let pasted_to_question = {
                    let mut app = state.lock().unwrap();
                    if let Some(question) = app.question.as_mut() {
                        if question.is_free_text() {
                            for character in text.chars() {
                                question.push_char(character);
                            }
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if !pasted_to_question {
                    ui.input.paste(&text);
                }
            }
            TerminalRoute::Help { dismiss } => {
                if dismiss {
                    *ui.help_visible = false;
                }
            }
            TerminalRoute::OpenHelp => *ui.help_visible = true,
            TerminalRoute::InputPage(key) => {
                effects.extend(Self::apply_input_page_key(
                    &key,
                    state,
                    &mut InputPageUiState {
                        input_page: ui.input_page,
                        input: ui.input,
                        config: ui.config,
                        themes: ui.themes,
                        theme: ui.theme,
                    },
                ));
            }
            TerminalRoute::Approval(key) => effects.extend(Self::answer_approval(&key, state)),
            TerminalRoute::Copy(key) => {
                let action = {
                    let app = state.lock().unwrap();
                    let rows = ui
                        .copy_rows_cache
                        .rows_with(&app, || crate::ui::copy_layout_rows(&app));
                    ui.copy_mode
                        .as_mut()
                        .expect("terminal router requires active copy mode")
                        .handle_key(&key, rows, &app)
                };
                match action {
                    copy::CopyAction::None => {}
                    copy::CopyAction::Exit => *ui.copy_mode = None,
                    copy::CopyAction::Copy(text) => {
                        *ui.copy_mode = None;
                        effects.push(RuntimeEffect::WriteClipboard(text));
                    }
                    copy::CopyAction::ToggleExpand(unit) => {
                        crate::presentation::toggle_expand(&mut state.lock().unwrap(), unit);
                    }
                    copy::CopyAction::Moved(global_row) => {
                        let visible = (terminal_height as usize).saturating_sub(5);
                        ui.scroll.follow = false;
                        let first = ui.scroll.offset;
                        let last = ui.scroll.offset + visible;
                        if global_row < first {
                            ui.scroll.offset = global_row;
                        } else if global_row >= last {
                            ui.scroll.offset = global_row.saturating_sub(visible) + 1;
                        }
                    }
                }
                if ui.copy_toast.as_ref().is_some_and(|(_, at)| {
                    now.saturating_duration_since(*at)
                        > Duration::from_secs(ui.config.copy_toast_secs)
                }) {
                    *ui.copy_toast = None;
                }
            }
            TerminalRoute::QuestionThenOrdinary(key) => {
                let (handled, question_effects) = Self::apply_question_key(&key, state);
                effects.extend(question_effects);
                if !handled {
                    effects.extend(Self::apply_ordinary_key(key, state, ui));
                }
            }
            TerminalRoute::Ordinary(key) => {
                effects.extend(Self::apply_ordinary_key(key, state, ui));
            }
            TerminalRoute::Ignore => {}
        }
        effects
    }

    fn apply_ordinary_key(
        key: KeyEvent,
        state: &Arc<Mutex<AppState>>,
        ui: &mut TerminalUiState<'_>,
    ) -> Vec<RuntimeEffect> {
        if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
            Self::apply_action(
                ControllerAction::OpenPage(InputPageSession::resume()),
                ui.input_page,
            );
            return vec![RuntimeEffect::Send(ClientMessage::ListSessions)];
        }

        let idle = {
            let app = state.lock().unwrap();
            app.is_new_conversation()
                || (app.status == crate::model::AgentStatus::Idle && !app.has_active_command())
        };
        let action = ui.input.handle_key(&key, idle);
        let mut outcome = Self::apply_input_action(action, state);
        if outcome.activate_copy_mode {
            *ui.copy_mode = Some(copy::CopyMode::default());
        }
        if let Some(line) = outcome.command {
            let command = runtime_command::handle_local_command(
                line,
                LocalCommandContext {
                    input_page: ui.input_page,
                    help_visible: ui.help_visible,
                    copy_mode: ui.copy_mode,
                    config: ui.config,
                    themes: ui.themes,
                    new_modes: &ui.input.new_modes,
                    input_paste_placeholder_chars: &mut ui.input.paste_placeholder_chars,
                    input_history_limit: &mut ui.input.history_limit,
                    theme: ui.theme,
                    state,
                },
            );
            if command.starts_interruptible_command {
                state.lock().unwrap().begin_command_execution();
            }
            outcome
                .effects
                .extend(command.outbound.into_iter().map(RuntimeEffect::Send));
            if command.new_conversation {
                *ui.scroll = ScrollState::default();
                *ui.copy_mode = None;
                *ui.input_page = None;
            }
            if command.reload_config {
                outcome.effects.push(RuntimeEffect::ReloadConfig);
            }
            if command.quit {
                outcome.effects.push(RuntimeEffect::Quit);
            }
        }
        outcome.effects
    }

    /// Apply one bridge frame and return deferred external effects.
    pub fn apply_bridge(
        msg: ServerMessage,
        state: &Arc<Mutex<AppState>>,
        ui: &mut BridgeUiState<'_>,
    ) -> Vec<RuntimeEffect> {
        match &msg {
            ServerMessage::Welcome {
                protocol_version,
                session_id,
                status,
                provider,
                model,
                mode,
                title,
                cwd,
                ..
            } => {
                if let Some(bridge_version) = protocol_version {
                    if *bridge_version != WIRE_PROTOCOL_VERSION {
                        return vec![RuntimeEffect::Fatal(protocol_mismatch_fatal(&format!(
                            "client protocol {WIRE_PROTOCOL_VERSION}, bridge protocol {bridge_version}"
                        )))];
                    }
                }
                let switched = {
                    let mut state = state.lock().unwrap();
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
                    *ui.scroll = ScrollState::default();
                    *ui.copy_mode = None;
                    ui.input.replace_integrated_commands(Vec::new());
                    ui.input.replace_skills(Vec::new());
                }
                vec![RuntimeEffect::PersistSessionId(session_id.clone())]
            }
            ServerMessage::Snapshot { events, truncated } => {
                let _zone = crate::tracy_zone!("snapshot apply");
                state.lock().unwrap().apply_snapshot(events, *truncated);
                Vec::new()
            }
            ServerMessage::Event { event } => {
                state.lock().unwrap().apply_host_event(event);
                Vec::new()
            }
            ServerMessage::History { events, has_more } => {
                let events = events.clone();
                let mut state = state.lock().unwrap();
                state.prepend_host_events(&events);
                state.history_loading = false;
                state.history_exhausted = !*has_more;
                Vec::new()
            }
            ServerMessage::Status { status } => {
                state
                    .lock()
                    .unwrap()
                    .apply("status", &serde_json::json!({ "status": status }));
                Vec::new()
            }
            ServerMessage::Sessions {
                sessions,
                titles_pending,
            } => {
                let sessions = sessions.clone();
                state.lock().unwrap().sessions = sessions.clone();
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_sessions(sessions, *titles_pending);
                }
                Vec::new()
            }
            ServerMessage::Presets { presets } => {
                ui.input.new_modes.clear();
                ui.input.new_modes.extend(
                    presets
                        .iter()
                        .filter(|preset| preset.broken.is_none())
                        .map(|preset| NewMode {
                            id: preset.id.clone(),
                            name: preset.name.clone(),
                            description: preset.description.clone(),
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
                Vec::new()
            }
            ServerMessage::Skills { skills } => {
                ui.input.replace_skills(skills.clone());
                Vec::new()
            }
            ServerMessage::Title { title } => {
                state.lock().unwrap().session_title = Some(title.clone());
                Vec::new()
            }
            ServerMessage::Commands { commands } => {
                ui.input.replace_integrated_commands(commands.clone());
                Vec::new()
            }
            ServerMessage::CommandResult {
                command_id,
                kind,
                text,
            } => {
                let mut state = state.lock().unwrap();
                state.finish_command_execution();
                state.apply_command_result(command_id, kind, text.as_deref());
                Vec::new()
            }
            ServerMessage::Login {
                providers,
                proxies,
                error,
            } => {
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_login(LoginView {
                        providers: providers.clone(),
                        proxies: proxies.clone(),
                        error: error.clone(),
                    });
                }
                Vec::new()
            }
            ServerMessage::Model { providers, current } => {
                let selected = current
                    .clone()
                    .map(|current| (current.provider.clone(), current.model.clone()));
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_model(providers.clone(), selected);
                }
                let mut state = state.lock().unwrap();
                if let Some(current) = current {
                    state.provider = Some(current.provider.clone());
                    state.model = Some(current.model.clone());
                }
                Vec::new()
            }
            ServerMessage::Approval {
                id,
                tool_name,
                reason,
                ..
            } => {
                state.lock().unwrap().approval = Some(ApprovalCard {
                    id: id.clone(),
                    tool_name: tool_name.clone(),
                    reason: reason.clone(),
                });
                Vec::new()
            }
            ServerMessage::Question {
                rpc_id,
                session_id,
                questions,
            } => {
                state.lock().unwrap().question = Some(QuestionBatch::new(
                    rpc_id.clone(),
                    session_id.clone(),
                    questions.clone(),
                ));
                Vec::new()
            }
            ServerMessage::QuestionResolved {
                question_rpc_id, ..
            } => {
                let mut state = state.lock().unwrap();
                if state
                    .question
                    .as_ref()
                    .map(|question| question.rpc_id.as_str())
                    == Some(question_rpc_id.as_str())
                {
                    state.question = None;
                }
                Vec::new()
            }
            ServerMessage::Error { code, message } => {
                if code == "disconnected" {
                    vec![RuntimeEffect::Fatal(format!(
                        "bridge disconnected: {message}"
                    ))]
                } else if code == "protocol-newer" {
                    vec![RuntimeEffect::Fatal(protocol_mismatch_fatal(message))]
                } else if code == "bad-token" {
                    vec![RuntimeEffect::Fatal(format!(
                        "bridge authentication failed: {message}"
                    ))]
                } else if code == "hello-failed" {
                    vec![RuntimeEffect::Fatal(format!(
                        "bridge startup failed: {message}"
                    ))]
                } else if code == "new-failed" {
                    let restored = {
                        let mut app = state.lock().unwrap();
                        let restored = app.restore_new_conversation_input();
                        if restored.is_some() {
                            app.set_new_conversation_notice(format!("创建新对话失败：{message}"));
                        }
                        restored
                    };
                    if let Some(text) = restored {
                        ui.input.buf = text;
                        ui.input.cursor = ui.input.buf.chars().count();
                        ui.input.pasted = false;
                        ui.input.multiline = ui.input.buf.contains('\n');
                    } else {
                        state
                            .lock()
                            .unwrap()
                            .push_error_message(format!("桥接错误 {code}: {message}"));
                    }
                    Vec::new()
                } else if code == "command-cancelled" {
                    state.lock().unwrap().finish_command_execution();
                    Vec::new()
                } else {
                    let mut state = state.lock().unwrap();
                    if matches!(
                        code.as_str(),
                        "no-commands"
                            | "command-unknown"
                            | "command-invalid-result"
                            | "command-failed"
                    ) {
                        state.finish_command_execution();
                    }
                    state.push_error_message(format!("桥接错误 {code}: {message}"));
                    Vec::new()
                }
            }
            ServerMessage::Pong => Vec::new(),
        }
    }

    pub fn apply_action(action: ControllerAction, input_page: &mut Option<InputPageSession>) {
        match action {
            ControllerAction::OpenPage(page) => *input_page = Some(page),
            ControllerAction::ClosePage => *input_page = None,
        }
    }

    pub fn apply_input_action(action: InputAction, state: &Mutex<AppState>) -> InputHandlerOutcome {
        let mut outcome = InputHandlerOutcome::default();
        match action {
            InputAction::None | InputAction::ToggleMultiline => {}
            InputAction::Send(text) => {
                let new_input = {
                    let mut state = state.lock().unwrap();
                    if state.is_new_conversation() {
                        state.materialize_new_conversation(text.clone())
                    } else {
                        None
                    }
                };
                if let Some(message) = new_input {
                    outcome.effects.push(RuntimeEffect::Send(message));
                    return outcome;
                }
                let is_draft = state.lock().unwrap().is_new_conversation();
                if is_draft {
                    state
                        .lock()
                        .unwrap()
                        .set_new_conversation_notice("正在创建新对话，请稍候");
                    return outcome;
                }
                let immediate = {
                    let mut state = state.lock().unwrap();
                    let immediate = state.enqueue_or_immediate(&text);
                    if immediate {
                        state.start_thinking();
                    }
                    immediate
                };
                if immediate {
                    outcome
                        .effects
                        .push(RuntimeEffect::Send(ClientMessage::Input { text }));
                }
            }
            InputAction::Command(line) => outcome.command = Some(line),
            InputAction::Interrupt => {
                state.lock().unwrap().queue.clear();
                outcome
                    .effects
                    .push(RuntimeEffect::Send(ClientMessage::Interrupt));
            }
            InputAction::Quit => outcome.effects.push(RuntimeEffect::Quit),
            InputAction::CopyMode => outcome.activate_copy_mode = true,
        }
        outcome
    }

    pub fn answer_approval(key: &KeyEvent, state: &Mutex<AppState>) -> Vec<RuntimeEffect> {
        let allow = matches!(key.code, KeyCode::Char('y' | 'Y'));
        let id = {
            let mut state = state.lock().unwrap();
            let Some(card) = state.approval.take() else {
                return Vec::new();
            };
            card.id
        };
        vec![RuntimeEffect::Send(ClientMessage::ApprovalAnswer {
            id,
            allow,
        })]
    }

    pub fn apply_question_key(
        key: &KeyEvent,
        state: &Mutex<AppState>,
    ) -> (bool, Vec<RuntimeEffect>) {
        let action = {
            let mut state = state.lock().unwrap();
            crate::input::handle_question_key(&mut state, key)
        };
        let effects = action
            .outbound
            .into_iter()
            .map(RuntimeEffect::Send)
            .collect();
        (action.handled, effects)
    }

    /// Apply one Input Page key synchronously, consume page-state actions, and
    /// return only lock-external work. Config persistence owns a cloned
    /// snapshot, so the runner never has to borrow controller state.
    pub fn apply_input_page_key(
        key: &KeyEvent,
        state: &Mutex<AppState>,
        ui: &mut InputPageUiState<'_>,
    ) -> Vec<RuntimeEffect> {
        let outcome = ui
            .input_page
            .as_mut()
            .expect("input-page handler requires an open page")
            .handle_key(key, ui.config);
        let mut effects = Vec::new();
        for effect in outcome.effects {
            match effect {
                PageEffect::Send(message) => effects.push(RuntimeEffect::Send(message)),
                PageEffect::ConfigChanged => {
                    ui.config.resolved_theme = theme::resolve(&ui.config.theme, ui.themes);
                    {
                        let mut state = state.lock().unwrap();
                        state.config = ui.config.clone();
                        state.markdown_layout.invalidate_all();
                        state.transcript_cache.invalidate();
                    }
                    *ui.theme = ui.config.theme();
                    ui.input.paste_placeholder_chars = ui.config.paste_placeholder_chars;
                    ui.input.history_limit = ui.config.history_limit;
                    effects.push(RuntimeEffect::PersistConfig(ui.config.clone()));
                }
            }
        }
        if outcome.close {
            Self::apply_action(ControllerAction::ClosePage, ui.input_page);
        }
        effects
    }

    pub fn apply_reloaded_config(
        config: Config,
        themes: Vec<ThemeFile>,
        state: &Mutex<AppState>,
        ui: &mut TerminalUiState<'_>,
    ) {
        *ui.config = config;
        *ui.themes = themes;
        *ui.theme = ui.config.theme();
        ui.input.paste_placeholder_chars = ui.config.paste_placeholder_chars;
        ui.input.history_limit = ui.config.history_limit;
        let mut state = state.lock().unwrap();
        state.config = ui.config.clone();
        state.markdown_layout.invalidate_all();
        state.transcript_cache.invalidate();
        state.push_system_message("已重载配置、主题与技能");
    }

    pub fn apply_effect_result(result: EffectResult, state: &Mutex<AppState>) {
        match result {
            EffectResult::ConfigPersisted(Ok(()))
            | EffectResult::ConfigReloaded { .. }
            | EffectResult::ClipboardWritten { .. } => {}
            EffectResult::ConfigPersisted(Err(error)) | EffectResult::ConfigReloadFailed(error) => {
                state
                    .lock()
                    .unwrap()
                    .push_error_message(format!("设置保存失败: {error}"));
            }
            EffectResult::ClipboardFailed(error) => {
                state
                    .lock()
                    .unwrap()
                    .push_error_message(format!("剪贴板写入失败: {error}"));
            }
        }
    }

    /// Atomically claim one queued prompt and defer transport I/O to the runner.
    pub fn dispatch_next_queued(state: &Mutex<AppState>) -> Vec<RuntimeEffect> {
        let mut state = state.lock().unwrap();
        let Some(text) = state.take_next_queued() else {
            return Vec::new();
        };
        state.start_thinking();
        vec![RuntimeEffect::Send(ClientMessage::Input { text })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, input::InputState};

    fn ui<'a>(
        scroll: &'a mut ScrollState,
        copy_mode: &'a mut Option<copy::CopyMode>,
        input: &'a mut InputState,
        input_page: &'a mut Option<InputPageSession>,
    ) -> BridgeUiState<'a> {
        BridgeUiState {
            scroll,
            copy_mode,
            input,
            input_page,
        }
    }

    #[test]
    fn welcome_switch_resets_agent_scoped_ui_and_defers_state_file_io() {
        let state = Arc::new(Mutex::new(AppState::default()));
        let mut scroll = ScrollState {
            follow: false,
            offset: 10,
        };
        let mut copy_mode = Some(copy::CopyMode::default());
        let mut input = InputState::new(&Config::default());
        input
            .integrated_commands
            .push(crate::protocol::CommandInfo {
                name: "plugin".into(),
                description: String::new(),
                input: None,
            });
        let mut page = None;
        let effects = RuntimeController::apply_bridge(
            ServerMessage::Welcome {
                protocol_version: Some(WIRE_PROTOCOL_VERSION),
                max_frame_bytes: None,
                session_id: "s1".into(),
                status: "idle".into(),
                provider: None,
                model: None,
                mode: Some("standard".into()),
                title: None,
                cwd: None,
            },
            &state,
            &mut ui(&mut scroll, &mut copy_mode, &mut input, &mut page),
        );
        assert!(scroll.follow && scroll.offset == 0);
        assert!(copy_mode.is_none());
        assert!(input.integrated_commands.is_empty());
        assert!(matches!(
            effects.as_slice(),
            [RuntimeEffect::PersistSessionId(id)] if id == "s1"
        ));
    }

    fn bridge_effects(message: ServerMessage) -> Vec<RuntimeEffect> {
        let state = Arc::new(Mutex::new(AppState::default()));
        let mut scroll = ScrollState::default();
        let mut copy_mode = None;
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        RuntimeController::apply_bridge(
            message,
            &state,
            &mut ui(&mut scroll, &mut copy_mode, &mut input, &mut page),
        )
    }

    fn bridge_error(code: &str, message: &str) -> Vec<RuntimeEffect> {
        bridge_effects(ServerMessage::Error {
            code: code.into(),
            message: message.into(),
        })
    }

    #[test]
    fn disconnected_error_is_a_deferred_fatal_effect() {
        let effects = bridge_error("disconnected", "gone");
        assert!(matches!(
            effects.as_slice(),
            [RuntimeEffect::Fatal(reason)] if reason.contains("gone")
        ));
    }

    #[test]
    fn handshake_errors_remain_actionable_before_the_close_frame() {
        let protocol = bridge_error(
            "protocol-newer",
            "client protocol 5 is newer than bridge protocol 4",
        );
        assert!(matches!(
            protocol.as_slice(),
            [RuntimeEffect::Fatal(reason)]
                if reason.contains("protocol mismatch")
                    && reason.contains("mount")
                    && reason.contains("rebuild")
                    && reason.contains("restart DSH")
        ));

        let newer_bridge = bridge_effects(ServerMessage::Welcome {
            protocol_version: Some(WIRE_PROTOCOL_VERSION + 1),
            max_frame_bytes: None,
            session_id: "ignored".into(),
            status: "idle".into(),
            provider: None,
            model: None,
            mode: None,
            title: None,
            cwd: None,
        });
        assert!(matches!(
            newer_bridge.as_slice(),
            [RuntimeEffect::Fatal(reason)]
                if reason.contains(&format!("client protocol {WIRE_PROTOCOL_VERSION}"))
                    && reason.contains(&format!("bridge protocol {}", WIRE_PROTOCOL_VERSION + 1))
        ));

        for (code, expected) in [
            ("bad-token", "authentication failed"),
            ("hello-failed", "startup failed"),
        ] {
            let effects = bridge_error(code, "rejected");
            assert!(matches!(
                effects.as_slice(),
                [RuntimeEffect::Fatal(reason)] if reason.contains(expected)
            ));
        }
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn terminal_router_preserves_global_and_blocking_priority() {
        let all_open = TerminalFocus {
            help_visible: false,
            input_page_open: true,
            question_open: true,
            approval_open: true,
            copy_mode_open: true,
        };
        assert!(matches!(
            route_terminal_event(key(KeyCode::PageUp), all_open),
            TerminalRoute::TranscriptPage { up: true }
        ));
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('x')), all_open),
            TerminalRoute::InputPage(_)
        ));

        let blocking = TerminalFocus {
            input_page_open: false,
            ..all_open
        };
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('y')), blocking),
            TerminalRoute::Copy(_)
        ));
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('x')), blocking),
            TerminalRoute::Copy(_)
        ));

        let approval = TerminalFocus {
            question_open: false,
            copy_mode_open: false,
            ..blocking
        };
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('n')), approval),
            TerminalRoute::Approval(_)
        ));
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('x')), approval),
            TerminalRoute::Ordinary(_)
        ));
    }

    #[test]
    fn help_and_paste_routes_are_terminal_owned() {
        let help = TerminalFocus {
            help_visible: true,
            ..TerminalFocus::default()
        };
        assert_eq!(
            route_terminal_event(key(KeyCode::Char('x')), help),
            TerminalRoute::Help { dismiss: false }
        );
        assert_eq!(
            route_terminal_event(Event::Paste("abc".into()), help),
            TerminalRoute::Paste { text: "abc".into() }
        );
    }

    #[test]
    fn esc_interrupts_a_command_while_the_agent_is_idle() {
        let state = Arc::new(Mutex::new(AppState::default()));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        input.buf = "/plugin slow".into();
        input.cursor = input.buf.chars().count();
        let mut input_page = None;
        let mut help_visible = false;
        let mut copy_mode = None;
        let mut rows_cache = copy::CopyRowsCache::default();
        let mut copy_toast = None;
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut theme = config.theme();

        let command = RuntimeController::apply_terminal_route(
            TerminalRoute::Ordinary(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            40,
            Instant::now(),
            &state,
            &mut TerminalUiState {
                scroll: &mut scroll,
                input: &mut input,
                input_page: &mut input_page,
                help_visible: &mut help_visible,
                copy_mode: &mut copy_mode,
                copy_rows_cache: &mut rows_cache,
                copy_toast: &mut copy_toast,
                config: &mut config,
                themes: &mut themes,
                theme: &mut theme,
            },
        );
        assert!(matches!(
            command.as_slice(),
            [RuntimeEffect::Send(ClientMessage::Command { line })] if line == "/plugin slow"
        ));
        assert!(state.lock().unwrap().has_active_command());

        let interrupt = RuntimeController::apply_terminal_route(
            TerminalRoute::Ordinary(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            40,
            Instant::now(),
            &state,
            &mut TerminalUiState {
                scroll: &mut scroll,
                input: &mut input,
                input_page: &mut input_page,
                help_visible: &mut help_visible,
                copy_mode: &mut copy_mode,
                copy_rows_cache: &mut rows_cache,
                copy_toast: &mut copy_toast,
                config: &mut config,
                themes: &mut themes,
                theme: &mut theme,
            },
        );
        assert!(matches!(
            interrupt.as_slice(),
            [RuntimeEffect::Send(ClientMessage::Interrupt)]
        ));

        let before = state.lock().unwrap().transcript.len();
        let mut bridge_ui = ui(&mut scroll, &mut copy_mode, &mut input, &mut input_page);
        RuntimeController::apply_bridge(
            ServerMessage::Error {
                code: "command-cancelled".into(),
                message: "command cancelled".into(),
            },
            &state,
            &mut bridge_ui,
        );
        let state = state.lock().unwrap();
        assert!(!state.has_active_command());
        assert_eq!(state.transcript.len(), before, "cancel ack stays silent");
    }

    #[test]
    fn page_actions_are_consumed_inside_the_controller_boundary() {
        let mut page = None;
        RuntimeController::apply_action(
            ControllerAction::OpenPage(InputPageSession::login()),
            &mut page,
        );
        assert!(page.is_some());
        RuntimeController::apply_action(ControllerAction::ClosePage, &mut page);
        assert!(page.is_none());
    }

    #[test]
    fn clipboard_failure_becomes_visible_after_effect_completion() {
        let state = Mutex::new(AppState::default());
        state.lock().unwrap().transcript_cache.valid = true;
        RuntimeController::apply_effect_result(
            EffectResult::ClipboardFailed("denied".into()),
            &state,
        );
        let state = state.lock().unwrap();
        assert!(matches!(
            state.msgs.last(),
            Some(Msg::Error { text }) if text.contains("denied")
        ));
        assert!(matches!(
            &state.transcript.nodes().last().unwrap().item,
            crate::display::DisplayItem::Block(block)
                if block.tone == crate::display::DisplayTone::Error
                    && block.content.contains("denied")
        ));
        assert!(!state.transcript_cache.valid);
    }

    #[test]
    fn copy_movement_route_releases_state_lock() {
        let mut app = AppState::default();
        app.msgs.push(Msg::Assistant {
            text: "a\nb".into(),
            lines: vec![
                crate::render::RenderLine {
                    line: ratatui::text::Line::from("a"),
                    unit: 1,
                    raw_line: Some(0),
                    atomic: false,
                    fill: false,
                },
                crate::render::RenderLine {
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
        let state = Arc::new(Mutex::new(app));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut input_page = None;
        let mut help_visible = false;
        let mut copy_mode = Some(copy::CopyMode::default());
        let mut rows_cache = copy::CopyRowsCache::default();
        let mut copy_toast = None;
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut theme = config.theme();
        RuntimeController::apply_terminal_route(
            TerminalRoute::Copy(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            40,
            Instant::now(),
            &state,
            &mut TerminalUiState {
                scroll: &mut scroll,
                input: &mut input,
                input_page: &mut input_page,
                help_visible: &mut help_visible,
                copy_mode: &mut copy_mode,
                copy_rows_cache: &mut rows_cache,
                copy_toast: &mut copy_toast,
                config: &mut config,
                themes: &mut themes,
                theme: &mut theme,
            },
        );
        assert_eq!(copy_mode.unwrap().cursor, 1);
        assert!(state.try_lock().is_ok());
    }

    #[test]
    fn config_reload_refreshes_runtime_theme_input_and_render_sidecars() {
        let loaded = Config::from_user_toml(
            r#"
                theme = "ferra"
                paste_placeholder_chars = 9
                history_limit = 77
            "#,
        )
        .unwrap();
        let state = Mutex::new(AppState::default());
        state.lock().unwrap().transcript_cache.valid = true;
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut input_page = None;
        let mut help_visible = false;
        let mut copy_mode = None;
        let mut rows_cache = copy::CopyRowsCache::default();
        let mut copy_toast = None;
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut runtime_theme = config.theme();
        RuntimeController::apply_reloaded_config(
            loaded,
            Vec::new(),
            &state,
            &mut TerminalUiState {
                scroll: &mut scroll,
                input: &mut input,
                input_page: &mut input_page,
                help_visible: &mut help_visible,
                copy_mode: &mut copy_mode,
                copy_rows_cache: &mut rows_cache,
                copy_toast: &mut copy_toast,
                config: &mut config,
                themes: &mut themes,
                theme: &mut runtime_theme,
            },
        );
        assert_eq!(config.theme, "ferra");
        assert_eq!(runtime_theme.user, Theme::ferra().user);
        assert_eq!(input.paste_placeholder_chars, 9);
        assert_eq!(input.history_limit, 77);
        let state = state.lock().unwrap();
        assert_eq!(state.config.theme, "ferra");
        assert!(!state.transcript_cache.valid);
    }

    #[test]
    fn draft_first_prompt_uses_atomic_new_input_without_old_queue() {
        let state = Mutex::new(AppState::default());
        state.lock().unwrap().begin_new_conversation("code");
        let outcome =
            RuntimeController::apply_input_action(InputAction::Send("first prompt".into()), &state);
        assert!(matches!(
            outcome.effects.as_slice(),
            [RuntimeEffect::Send(ClientMessage::NewInput { mode, text })]
                if mode == "code" && text == "first prompt"
        ));
        let state = state.lock().unwrap();
        assert!(state.queue.is_empty());
        assert_eq!(
            state
                .new_conversation
                .as_ref()
                .and_then(|draft| draft.pending_input.as_deref()),
            Some("first prompt")
        );
    }

    #[test]
    fn new_failure_restores_the_retained_first_prompt() {
        let state = Arc::new(Mutex::new(AppState::default()));
        state.lock().unwrap().begin_new_conversation("standard");
        let _ = RuntimeController::apply_input_action(InputAction::Send("retry me".into()), &state);
        let mut scroll = ScrollState::default();
        let mut copy_mode = None;
        let mut input = InputState::new(&Config::default());
        let mut input_page = None;
        let effects = RuntimeController::apply_bridge(
            ServerMessage::Error {
                code: "new-failed".into(),
                message: "creation failed".into(),
            },
            &state,
            &mut BridgeUiState {
                scroll: &mut scroll,
                copy_mode: &mut copy_mode,
                input: &mut input,
                input_page: &mut input_page,
            },
        );
        assert!(effects.is_empty());
        assert_eq!(input.buf, "retry me");
        assert_eq!(input.cursor, 8);
        let app = state.lock().unwrap();
        assert!(app.is_new_conversation());
        assert!(app
            .new_conversation
            .as_ref()
            .and_then(|draft| draft.notice.as_deref())
            .is_some_and(|notice| notice.contains("creation failed")));
    }

    #[test]
    fn queued_dispatch_returns_send_after_atomic_state_change() {
        let state = Mutex::new(AppState::default());
        state.lock().unwrap().queue.push("next".into());
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(state.lock().unwrap().queue.is_empty());
        assert!(matches!(
            effects.as_slice(),
            [RuntimeEffect::Send(ClientMessage::Input { text })] if text == "next"
        ));
    }
}
