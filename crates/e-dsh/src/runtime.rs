//! Testable runtime inputs, effects, and bridge-message controller.
//!
//! The Tokio loop in `main.rs` owns waiting and concrete I/O. This module
//! mutates typed client state and returns effects that the runner executes only
//! after state guards have been released.

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
#[cfg(test)]
use e_tui::AgentRequest;
use e_tui::{
    command_catalog::NewMode,
    input::{InputAction, InputState},
    input_page::{InputPageSession, PageEffect},
    login::LoginView,
    ui::{scroll_lines, scroll_page, transcript_view_height, ScrollState, TerminalSize},
    AgentEvent, MouseSelection, NoticeState, PointerEvent, SelectionFrame,
};
pub use e_tui::{DrawPriority, EffectResult, UiAction};

#[cfg(test)]
use crate::model::Msg;
use crate::{
    config::Config,
    model::{AppState, ApprovalCard, QuestionBatch},
    protocol::{ClientMessage, ServerMessage, WIRE_PROTOCOL_VERSION},
    runtime_command::{self, LocalCommandContext},
    theme::{self, Theme, ThemeFile},
};

pub enum RuntimeInput {
    Bridge(AgentEvent),
    Terminal(Event),
    EffectCompleted(EffectResult),
    AnimationDeadline,
    FrameDeadline,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalFocus {
    pub help_visible: bool,
    pub input_page_open: bool,
    pub approval_open: bool,
    pub reading_view_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalRoute {
    Pointer(PointerEvent),
    Paste { text: String },
    Help { dismiss: bool },
    OpenHelp,
    TranscriptPage { up: bool },
    InputPage(KeyEvent),
    Approval(KeyEvent),
    Reading(KeyEvent),
    Ordinary(KeyEvent),
    Ignore,
}

/// Encode terminal ownership and precedence independently of terminal I/O.
/// Input Pages own all keys while open; approval keeps its compact y/n route.
pub fn route_terminal_event(event: Event, focus: TerminalFocus) -> TerminalRoute {
    match event {
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => TerminalRoute::Pointer(PointerEvent::Wheel { up: true }),
            MouseEventKind::ScrollDown => TerminalRoute::Pointer(PointerEvent::Wheel { up: false }),
            MouseEventKind::Down(MouseButton::Left) => {
                TerminalRoute::Pointer(PointerEvent::PrimaryPress {
                    column: mouse.column,
                    row: mouse.row,
                })
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                TerminalRoute::Pointer(PointerEvent::PrimaryDrag {
                    column: mouse.column,
                    row: mouse.row,
                })
            }
            MouseEventKind::Up(MouseButton::Left) => {
                TerminalRoute::Pointer(PointerEvent::PrimaryRelease {
                    column: mouse.column,
                    row: mouse.row,
                })
            }
            _ => TerminalRoute::Ignore,
        },
        Event::FocusLost | Event::Resize(_, _) => TerminalRoute::Pointer(PointerEvent::FocusLost),
        Event::FocusGained => TerminalRoute::Ignore,
        Event::Paste(_) if focus.reading_view_open => TerminalRoute::Ignore,
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
        Event::Key(key) if focus.approval_open => TerminalRoute::Approval(key),
        Event::Key(key) if focus.reading_view_open => TerminalRoute::Reading(key),
        Event::Key(key) => TerminalRoute::Ordinary(key),
    }
}

/// Pure state transitions produced by handlers and consumed by the controller
/// while it owns the relevant UI state. These never reach infrastructure.
pub enum ControllerAction {
    OpenPage(InputPageSession),
    ClosePage,
}

fn agent_action(message: ClientMessage) -> UiAction {
    let request = crate::bridge::adapter::client_message_to_agent_request(message)
        .expect("hello is emitted only by the DSH transport composition root");
    UiAction::Agent(request)
}

/// Mutable UI-local state affected by bridge frames. Keeping it separate from
/// AppState makes session-switch behavior explicit without giving the bridge
/// handler ownership of terminal or transport infrastructure. The interaction
/// fields (approval/question/queue) are borrowed from the same InteractionModel
/// the terminal path uses, so bridge frames and key handling mutate the same
/// object — never a transient default.
pub struct BridgeUiState<'a> {
    pub scroll: &'a mut ScrollState,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
    pub approval: &'a mut Option<ApprovalCard>,
    pub question: &'a mut Option<String>,
    pub queue: &'a mut Vec<String>,
}

#[derive(Default)]
pub struct InputHandlerOutcome {
    pub command: Option<String>,
    pub activate_reading: bool,
    pub effects: Vec<UiAction>,
}

pub struct InputPageUiState<'a> {
    pub input_page: &'a mut Option<InputPageSession>,
    pub input: &'a mut InputState,
    pub config: &'a mut Config,
    pub themes: &'a [ThemeFile],
    pub theme: &'a mut Theme,
    pub question: &'a mut Option<String>,
}

pub struct TerminalUiState<'a> {
    pub scroll: &'a mut ScrollState,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
    pub help_visible: &'a mut bool,
    pub notice: &'a mut NoticeState,
    pub mouse_selection: &'a mut MouseSelection,
    pub approval: &'a mut Option<ApprovalCard>,
    pub question: &'a mut Option<String>,
    pub queue: &'a mut Vec<String>,
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
        size: TerminalSize,
        now: Instant,
        state: &Arc<Mutex<AppState>>,
        selection_frame: &SelectionFrame,
        ui: &mut TerminalUiState<'_>,
    ) -> Vec<UiAction> {
        let mut effects = Vec::new();
        match route {
            route @ (TerminalRoute::Pointer(PointerEvent::Wheel { up })
            | TerminalRoute::TranscriptPage { up }) => {
                let page = matches!(route, TerminalRoute::TranscriptPage { .. });
                let before = {
                    let mut app = state.lock().unwrap();
                    let height = transcript_view_height(
                        size,
                        &app,
                        ui.input,
                        ui.input_page.is_some(),
                        ui.approval.as_ref(),
                        ui.queue,
                    );
                    if page {
                        scroll_page(
                            ui.scroll,
                            height,
                            app.render.transcript_cache.display_len(),
                            up,
                        );
                    } else {
                        scroll_lines(
                            ui.scroll,
                            height,
                            app.render.transcript_cache.display_len(),
                            up,
                            3,
                        );
                    }
                    if up
                        && ui.scroll.offset == 0
                        && !ui.scroll.follow
                        && !app.session.history_exhausted
                        && !app.session.history_loading
                    {
                        app.session.min_seq.map(|seq| {
                            app.session.history_loading = true;
                            seq
                        })
                    } else {
                        None
                    }
                };
                if let Some(before_seq) = before {
                    effects.push(agent_action(ClientMessage::History {
                        before_seq,
                        limit: 400,
                    }));
                }
            }
            TerminalRoute::Pointer(pointer) => {
                if !selection_frame.matches_viewport(size.width, size.height) {
                    ui.mouse_selection.clear();
                    return effects;
                }
                let update = ui.mouse_selection.handle(pointer, selection_frame);
                if let Some(text) = update.copy {
                    effects.push(UiAction::WriteClipboard(text));
                }
            }
            TerminalRoute::Paste { text } => {
                if ui.input_page.as_mut().is_some_and(|page| page.paste(&text)) {
                    return effects;
                }
                ui.input.paste(&text);
            }
            TerminalRoute::Help { dismiss } => {
                if dismiss {
                    *ui.help_visible = false;
                }
            }
            TerminalRoute::OpenHelp => {
                ui.mouse_selection.clear();
                *ui.help_visible = true;
            }
            TerminalRoute::InputPage(key) => {
                ui.mouse_selection.clear();
                effects.extend(Self::apply_input_page_key(
                    &key,
                    state,
                    &mut InputPageUiState {
                        input_page: ui.input_page,
                        input: ui.input,
                        config: ui.config,
                        themes: ui.themes,
                        theme: ui.theme,
                        question: ui.question,
                    },
                ));
            }
            TerminalRoute::Approval(key) => {
                ui.mouse_selection.clear();
                effects.extend(Self::answer_approval(&key, ui.approval))
            }
            TerminalRoute::Reading(key) => {
                ui.mouse_selection.clear();
                effects.extend(Self::apply_reading_key(&key, size, state, ui));
            }
            TerminalRoute::Ordinary(key) => {
                ui.mouse_selection.clear();
                effects.extend(Self::apply_ordinary_key(key, size, now, state, ui));
            }
            TerminalRoute::Ignore => {}
        }
        effects
    }

    fn apply_reading_key(
        key: &KeyEvent,
        size: TerminalSize,
        state: &Arc<Mutex<AppState>>,
        ui: &mut TerminalUiState<'_>,
    ) -> Vec<UiAction> {
        let viewport_height = {
            let app = state.lock().unwrap();
            transcript_view_height(
                size,
                &app,
                ui.input,
                ui.input_page.is_some(),
                ui.approval.as_ref(),
                ui.queue,
            )
        };
        let item_mode = state
            .lock()
            .unwrap()
            .reading
            .as_ref()
            .is_some_and(|reading| reading.item_cursor.is_some());
        match key.code {
            KeyCode::Esc => {
                let mut app = state.lock().unwrap();
                if !app.leave_reading_items() {
                    app.exit_reading(ui.input);
                }
                app.take_actions()
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                let mut app = state.lock().unwrap();
                if item_mode {
                    app.move_reading_item(
                        e_tui::ReadingDirection::Down,
                        ui.scroll,
                        viewport_height,
                    );
                } else {
                    app.move_reading_block(1, ui.scroll, viewport_height);
                }
                app.take_actions()
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                let mut app = state.lock().unwrap();
                if item_mode {
                    app.move_reading_item(e_tui::ReadingDirection::Up, ui.scroll, viewport_height);
                } else {
                    app.move_reading_block(-1, ui.scroll, viewport_height);
                }
                app.take_actions()
            }
            KeyCode::Left | KeyCode::Char('h') if key.modifiers.is_empty() && item_mode => {
                let mut app = state.lock().unwrap();
                app.move_reading_item(e_tui::ReadingDirection::Left, ui.scroll, viewport_height);
                app.take_actions()
            }
            KeyCode::Right | KeyCode::Char('l') if key.modifiers.is_empty() => {
                let mut app = state.lock().unwrap();
                if item_mode {
                    app.move_reading_item(
                        e_tui::ReadingDirection::Right,
                        ui.scroll,
                        viewport_height,
                    );
                } else {
                    app.enter_reading_items();
                }
                app.take_actions()
            }
            KeyCode::Char('y') if key.modifiers.is_empty() => state
                .lock()
                .unwrap()
                .reading_copy_text()
                .map(|text| vec![UiAction::WriteClipboard(text)])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn apply_ordinary_key(
        key: KeyEvent,
        size: TerminalSize,
        now: Instant,
        state: &Arc<Mutex<AppState>>,
        ui: &mut TerminalUiState<'_>,
    ) -> Vec<UiAction> {
        if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
            Self::apply_action(
                ControllerAction::OpenPage(InputPageSession::resume()),
                ui.input_page,
            );
            return vec![agent_action(ClientMessage::ListSessions)];
        }

        let (idle, catalogs) = {
            let app = state.lock().unwrap();
            (
                app.is_new_conversation()
                    || (app.session.status == crate::model::AgentStatus::Idle
                        && !app.has_active_command()),
                app.catalogs.clone(),
            )
        };
        let action = ui.input.handle_key_with_catalog(&key, idle, &catalogs);
        let mut outcome = Self::apply_input_action(action, state, ui.queue);
        if outcome.activate_reading {
            let viewport_height = {
                let app = state.lock().unwrap();
                transcript_view_height(
                    size,
                    &app,
                    ui.input,
                    ui.input_page.is_some(),
                    ui.approval.as_ref(),
                    ui.queue,
                )
            };
            let (entered, actions) = {
                let mut app = state.lock().unwrap();
                let entered = app.enter_reading(ui.input, ui.scroll, viewport_height);
                (entered, app.take_actions())
            };
            outcome.effects.extend(actions);
            if !entered {
                ui.notice.show("没有可阅读的内容", now);
            }
        }
        if let Some(line) = outcome.command {
            let command = runtime_command::handle_local_command(
                line,
                LocalCommandContext {
                    input_page: ui.input_page,
                    help_visible: ui.help_visible,
                    config: ui.config,
                    themes: ui.themes,
                    new_modes: &catalogs.new_modes,
                    input_paste_placeholder_chars: &mut ui.input.paste_placeholder_chars,
                    input_history_limit: &mut ui.input.history_limit,
                    theme: ui.theme,
                    question_open: ui.question.is_some(),
                    approval_open: ui.approval.is_some(),
                    state,
                },
            );
            if command.starts_interruptible_command {
                state.lock().unwrap().begin_command_execution();
            }
            outcome
                .effects
                .extend(command.outbound.into_iter().map(agent_action));
            if command.activate_reading {
                let viewport_height = {
                    let app = state.lock().unwrap();
                    transcript_view_height(
                        size,
                        &app,
                        ui.input,
                        ui.input_page.is_some(),
                        ui.approval.as_ref(),
                        ui.queue,
                    )
                };
                let mut app = state.lock().unwrap();
                if !app.enter_reading(ui.input, ui.scroll, viewport_height) {
                    ui.notice.show("没有可阅读的内容", now);
                }
                outcome.effects.extend(app.take_actions());
            }
            if command.new_conversation {
                *ui.scroll = ScrollState::default();
                *ui.input_page = None;
                ui.mouse_selection.clear();
            }
            if command.reload_config {
                outcome.effects.push(UiAction::ReloadConfig);
            }
            if command.quit {
                outcome.effects.push(UiAction::Quit);
            }
        }
        outcome.effects
    }

    /// Apply one normalized agent fact and return deferred external effects.
    /// Legacy state projection remains in `e-dsh` during this migration phase.
    pub fn apply_agent(
        event: AgentEvent,
        state: &Arc<Mutex<AppState>>,
        ui: &mut BridgeUiState<'_>,
    ) -> Vec<UiAction> {
        match event {
            AgentEvent::Timeline(e_tui::agent::TimelineEvent::Snapshot { records, truncated }) => {
                let _zone = crate::tracy_zone!("snapshot apply");
                let mut app = state.lock().unwrap();
                app.apply_snapshot(&records, truncated);
                return app.take_actions();
            }
            AgentEvent::Timeline(e_tui::agent::TimelineEvent::Append(record)) => {
                let mut app = state.lock().unwrap();
                app.apply_host_event(&record);
                return app.take_actions();
            }
            AgentEvent::Timeline(e_tui::agent::TimelineEvent::History { records, has_more }) => {
                let mut state = state.lock().unwrap();
                state.prepend_host_events(&records);
                state.session.history_loading = false;
                state.session.history_exhausted = !has_more;
                return state.take_actions();
            }
            AgentEvent::Preview(e_tui::agent::PreviewEvent::Resolved {
                request_id,
                key,
                revision,
                result,
            }) => {
                let visible = state
                    .lock()
                    .unwrap()
                    .preview
                    .complete(request_id, key, revision, result);
                return visible
                    .then(|| UiAction::RequestDraw(DrawPriority::Content))
                    .into_iter()
                    .collect();
            }
            AgentEvent::EffectCompleted(result) => {
                let dirty = Self::apply_effect_result(result, state, Instant::now());
                return dirty
                    .then(|| UiAction::RequestDraw(DrawPriority::Content))
                    .into_iter()
                    .collect();
            }
            event => {
                let Ok(msg) = crate::bridge::adapter::legacy_server_message(event) else {
                    return Vec::new();
                };
                return Self::apply_legacy_message(&msg, state, ui);
            }
        }
    }

    fn apply_legacy_message(
        msg: &ServerMessage,
        state: &Arc<Mutex<AppState>>,
        ui: &mut BridgeUiState<'_>,
    ) -> Vec<UiAction> {
        match msg {
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
                        return vec![UiAction::Fatal(protocol_mismatch_fatal(&format!(
                            "client protocol {WIRE_PROTOCOL_VERSION}, bridge protocol {bridge_version}"
                        )))];
                    }
                }
                let switched = {
                    let mut state = state.lock().unwrap();
                    let switched = state.session.session_id.as_deref() != Some(session_id.as_str());
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
                    if ui
                        .input_page
                        .as_ref()
                        .is_some_and(|page| page.question_rpc_id().is_some())
                    {
                        *ui.input_page = None;
                    }
                    // Pending interaction belongs to the session where it was
                    // created and must never accept input after a switch.
                    *ui.approval = None;
                    *ui.question = None;
                    ui.queue.clear();
                    let catalogs = {
                        let mut app = state.lock().unwrap();
                        app.catalogs.integrated_commands.clear();
                        app.catalogs.skills.clear();
                        app.catalogs.clone()
                    };
                    ui.input.catalog_changed(&catalogs);
                }
                vec![UiAction::PersistSessionId(session_id.clone())]
            }
            ServerMessage::Snapshot { events, truncated } => {
                let _zone = crate::tracy_zone!("snapshot apply");
                let records = events
                    .iter()
                    .cloned()
                    .map(crate::bridge::adapter::normalize_host_event)
                    .collect::<Vec<_>>();
                state.lock().unwrap().apply_snapshot(&records, *truncated);
                Vec::new()
            }
            ServerMessage::Event { event } => {
                let record = crate::bridge::adapter::normalize_host_event(event.clone());
                state.lock().unwrap().apply_host_event(&record);
                Vec::new()
            }
            ServerMessage::History { events, has_more } => {
                let records = events
                    .iter()
                    .cloned()
                    .map(crate::bridge::adapter::normalize_host_event)
                    .collect::<Vec<_>>();
                let mut state = state.lock().unwrap();
                state.prepend_host_events(&records);
                state.session.history_loading = false;
                state.session.history_exhausted = !*has_more;
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
                let sessions = sessions
                    .iter()
                    .map(|session| e_tui::agent::SessionSummary {
                        id: session.id.clone(),
                        title: session.title.clone(),
                        live: session.live,
                        created_at: session.created_at,
                    })
                    .collect::<Vec<_>>();
                state.lock().unwrap().catalogs.sessions = sessions.clone();
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_sessions(sessions, *titles_pending);
                }
                Vec::new()
            }
            ServerMessage::Presets { presets } => {
                let modes = presets
                    .iter()
                    .filter(|preset| preset.broken.is_none())
                    .map(|preset| NewMode {
                        id: preset.id.clone(),
                        name: preset.name.clone(),
                        description: preset.description.clone(),
                    })
                    .collect::<Vec<_>>();
                let catalogs = {
                    let mut app = state.lock().unwrap();
                    app.catalogs.new_modes = modes.clone();
                    app.catalogs.clone()
                };
                ui.input.catalog_changed(&catalogs);
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_modes(modes.iter().map(|mode| mode.id.clone()).collect());
                }
                Vec::new()
            }
            ServerMessage::Skills { skills } => {
                let skills = skills
                    .iter()
                    .map(|skill| e_tui::agent::Skill {
                        name: skill.name.clone(),
                        description: skill.description.clone(),
                    })
                    .collect();
                let catalogs = {
                    let mut app = state.lock().unwrap();
                    app.catalogs.skills = skills;
                    app.catalogs.clone()
                };
                ui.input.catalog_changed(&catalogs);
                Vec::new()
            }
            ServerMessage::Title { title } => {
                state.lock().unwrap().session.session_title = Some(title.clone());
                Vec::new()
            }
            ServerMessage::Commands { commands } => {
                let commands = commands
                    .iter()
                    .map(|command| e_tui::agent::CommandDescriptor {
                        name: command.name.clone(),
                        description: command.description.clone(),
                        input_hint: command.input.as_ref().map(|input| input.hint.clone()),
                    })
                    .collect();
                let catalogs = {
                    let mut app = state.lock().unwrap();
                    app.catalogs.integrated_commands = commands;
                    app.catalogs.clone()
                };
                ui.input.catalog_changed(&catalogs);
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
                let providers = providers
                    .iter()
                    .map(|provider| e_tui::agent::CredentialProvider {
                        id: provider.id.clone(),
                        name: provider.name.clone(),
                        api_key_configured: provider.api_key_configured,
                        api_key_writable: provider.api_key_writable,
                        api_key_source: provider.api_key_source.clone(),
                        api_key_hint: provider.api_key_hint.clone(),
                    })
                    .collect::<Vec<_>>();
                let proxies = proxies
                    .iter()
                    .map(|proxy| e_tui::agent::ProxyRoute {
                        id: proxy.id.clone(),
                        name: proxy.name.clone(),
                        base_url: proxy.base_url.clone(),
                        protocol: proxy.protocol.clone(),
                        model: proxy.model.clone(),
                    })
                    .collect::<Vec<_>>();
                {
                    let mut app = state.lock().unwrap();
                    app.catalogs.credential_providers = providers.clone();
                    app.catalogs.proxies = proxies.clone();
                }
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_login(LoginView {
                        providers,
                        proxies,
                        error: error.clone(),
                    });
                }
                Vec::new()
            }
            ServerMessage::Model { providers, current } => {
                let providers = providers
                    .iter()
                    .map(|provider| e_tui::agent::ModelProvider {
                        id: provider.id.clone(),
                        name: provider.name.clone(),
                        models: provider
                            .models
                            .iter()
                            .map(|model| e_tui::agent::ModelDescriptor {
                                id: model.id.clone(),
                                name: model.name.clone(),
                                description: model.description.clone(),
                            })
                            .collect(),
                    })
                    .collect::<Vec<_>>();
                let selected = current
                    .clone()
                    .map(|current| (current.provider.clone(), current.model.clone()));
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_model(providers.clone(), selected);
                }
                let mut app = state.lock().unwrap();
                app.catalogs.model_providers = providers;
                app.catalogs.current_model =
                    current
                        .as_ref()
                        .map(|current| e_tui::agent::ModelSelection {
                            provider: current.provider.clone(),
                            model: current.model.clone(),
                        });
                if let Some(current) = current {
                    app.session.provider = Some(current.provider.clone());
                    app.session.model = Some(current.model.clone());
                }
                Vec::new()
            }
            ServerMessage::Approval {
                id,
                tool_name,
                reason,
                ..
            } => {
                *ui.approval = Some(ApprovalCard {
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
                let batch = QuestionBatch::new(
                    rpc_id.clone(),
                    session_id.clone(),
                    questions
                        .iter()
                        .map(|question| e_tui::agent::Question {
                            id: question.id.clone(),
                            question: question.question.clone(),
                            header: question.header.clone(),
                            options: question.options.as_ref().map(|options| {
                                options
                                    .iter()
                                    .map(|option| e_tui::agent::QuestionOption {
                                        label: option.label.clone(),
                                        description: option.description.clone(),
                                    })
                                    .collect()
                            }),
                            multi_select: question.multi_select,
                        })
                        .collect(),
                );
                *ui.question = Some(rpc_id.clone());
                *ui.input_page = Some(InputPageSession::question(batch));
                Vec::new()
            }
            ServerMessage::QuestionResolved {
                question_rpc_id, ..
            } => {
                if ui.question.as_deref() == Some(question_rpc_id.as_str()) {
                    *ui.question = None;
                }
                if ui
                    .input_page
                    .as_ref()
                    .and_then(InputPageSession::question_rpc_id)
                    == Some(question_rpc_id.as_str())
                {
                    *ui.input_page = None;
                }
                Vec::new()
            }
            ServerMessage::Error { code, message } => {
                if code == "disconnected" {
                    vec![UiAction::Fatal(format!("bridge disconnected: {message}"))]
                } else if code == "protocol-newer" {
                    vec![UiAction::Fatal(protocol_mismatch_fatal(message))]
                } else if code == "bad-token" {
                    vec![UiAction::Fatal(format!(
                        "bridge authentication failed: {message}"
                    ))]
                } else if code == "hello-failed" {
                    vec![UiAction::Fatal(format!("bridge startup failed: {message}"))]
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
                        ui.input.restore_text(text);
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

    pub fn apply_input_action(
        action: InputAction,
        state: &Mutex<AppState>,
        queue: &mut Vec<String>,
    ) -> InputHandlerOutcome {
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
                    outcome.effects.push(agent_action(message));
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
                    let immediate = state.enqueue_or_immediate(&text, queue);
                    if immediate {
                        state.start_thinking();
                    }
                    immediate
                };
                if immediate {
                    outcome
                        .effects
                        .push(agent_action(ClientMessage::Input { text }));
                }
            }
            InputAction::Command(line) => outcome.command = Some(line),
            InputAction::Interrupt => {
                queue.clear();
                outcome.effects.push(agent_action(ClientMessage::Interrupt));
            }
            InputAction::Quit => outcome.effects.push(UiAction::Quit),
            InputAction::PreviewToggle => {
                let mut app = state.lock().unwrap();
                app.preview.fullscreen = !app.preview.fullscreen;
            }
            InputAction::ReadingToggle => outcome.activate_reading = true,
        }
        outcome
    }

    pub fn answer_approval(key: &KeyEvent, approval: &mut Option<ApprovalCard>) -> Vec<UiAction> {
        let allow = matches!(key.code, KeyCode::Char('y' | 'Y'));
        let Some(card) = approval.take() else {
            return Vec::new();
        };
        vec![UiAction::Agent(card.answer(allow))]
    }

    /// Apply one Input Page key synchronously, consume page-state actions, and
    /// return only lock-external work. Config persistence owns a cloned
    /// snapshot, so the runner never has to borrow controller state.
    pub fn apply_input_page_key(
        key: &KeyEvent,
        state: &Mutex<AppState>,
        ui: &mut InputPageUiState<'_>,
    ) -> Vec<UiAction> {
        let was_question = ui
            .input_page
            .as_ref()
            .is_some_and(|page| page.question_rpc_id().is_some());
        let outcome = ui
            .input_page
            .as_mut()
            .expect("input-page handler requires an open page")
            .handle_key(key, ui.config);
        if was_question {
            let question = if outcome.close {
                None
            } else {
                ui.input_page
                    .as_ref()
                    .and_then(InputPageSession::question_rpc_id)
                    .map(str::to_owned)
            };
            *ui.question = question;
        }
        let mut effects = Vec::new();
        for effect in outcome.effects {
            match effect {
                PageEffect::Send(message) => effects.push(UiAction::Agent(message)),
                PageEffect::ConfigChanged => {
                    ui.config.resolved_theme = theme::resolve(&ui.config.theme, ui.themes);
                    {
                        let mut state = state.lock().unwrap();
                        state.config = ui.config.clone();
                        state.render.markdown_layout.invalidate_all();
                        state.render.transcript_cache.invalidate();
                    }
                    *ui.theme = ui.config.theme();
                    ui.input.paste_placeholder_chars = ui.config.paste_placeholder_chars;
                    ui.input.history_limit = ui.config.history_limit;
                    effects.push(UiAction::PersistConfig(ui.config.clone()));
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
        state.render.markdown_layout.invalidate_all();
        state.render.transcript_cache.invalidate();
        state.push_system_message("已重载配置、主题与技能");
    }

    pub fn apply_effect_result(
        result: EffectResult,
        state: &Mutex<AppState>,
        now: Instant,
    ) -> bool {
        match result {
            EffectResult::ConfigPersisted(Ok(())) | EffectResult::ConfigReloaded { .. } => false,
            EffectResult::ClipboardWritten {
                lines,
                preview,
                truncated,
            } => {
                state
                    .lock()
                    .unwrap()
                    .interaction
                    .notice
                    .show_clipboard(lines, &preview, truncated, now);
                true
            }
            EffectResult::ConfigPersisted(Err(error)) | EffectResult::ConfigReloadFailed(error) => {
                state
                    .lock()
                    .unwrap()
                    .push_error_message(format!("设置保存失败: {error}"));
                true
            }
            EffectResult::ClipboardFailed(error) => {
                let mut app = state.lock().unwrap();
                app.push_error_message(format!("剪贴板写入失败: {error}"));
                if app.reading.is_some() {
                    let placeholder = InputState::new(&app.config);
                    let mut input = std::mem::replace(&mut app.interaction.input, placeholder);
                    app.exit_reading(&mut input);
                    app.interaction.input = input;
                }
                true
            }
            EffectResult::PreviewResolved {
                request_id,
                key,
                revision,
                result,
            } => state
                .lock()
                .unwrap()
                .preview
                .complete(request_id, key, revision, result),
        }
    }

    /// Atomically claim one queued prompt and defer transport I/O to the runner.
    pub fn dispatch_next_queued(state: &Mutex<AppState>) -> Vec<UiAction> {
        let mut state = state.lock().unwrap();
        let Some(text) = state.take_next_queued() else {
            return Vec::new();
        };
        state.start_thinking();
        vec![agent_action(ClientMessage::Input { text })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn ui<'a>(
        scroll: &'a mut ScrollState,
        input: &'a mut InputState,
        input_page: &'a mut Option<InputPageSession>,
        approval: &'a mut Option<ApprovalCard>,
        question: &'a mut Option<String>,
        queue: &'a mut Vec<String>,
    ) -> BridgeUiState<'a> {
        BridgeUiState {
            scroll,
            input,
            input_page,
            approval,
            question,
            queue,
        }
    }

    fn apply_bridge(
        message: ServerMessage,
        state: &Arc<Mutex<AppState>>,
        ui: &mut BridgeUiState<'_>,
    ) -> Vec<UiAction> {
        RuntimeController::apply_agent(
            crate::bridge::adapter::normalize_server_message(message),
            state,
            ui,
        )
    }

    #[test]
    fn welcome_switch_resets_agent_scoped_ui_and_defers_state_file_io() {
        let state = Arc::new(Mutex::new(AppState::default()));
        let mut scroll = ScrollState {
            follow: false,
            offset: 10,
        };
        let mut input = InputState::new(&Config::default());
        state
            .lock()
            .unwrap()
            .catalogs
            .integrated_commands
            .push(e_tui::agent::CommandDescriptor {
                name: "plugin".into(),
                description: String::new(),
                input_hint: None,
            });
        let mut page = None;
        let mut approval = Some(ApprovalCard {
            id: "old-approval".into(),
            tool_name: "bash".into(),
            reason: "old session".into(),
        });
        let mut question = Some("old-question".into());
        let mut queue = vec!["old prompt".into()];
        let effects = apply_bridge(
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
            &mut ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert!(scroll.follow && scroll.offset == 0);
        assert!(approval.is_none());
        assert!(question.is_none());
        assert!(queue.is_empty());
        assert!(state
            .lock()
            .unwrap()
            .catalogs
            .integrated_commands
            .is_empty());
        assert!(matches!(
            effects.as_slice(),
            [UiAction::PersistSessionId(id)] if id == "s1"
        ));
    }

    #[test]
    fn question_frame_opens_input_page_without_consuming_input_buffer() {
        let state = Arc::new(Mutex::new(AppState::default()));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        input.buf = "draft prompt".into();
        input.cursor = input.buf.chars().count();
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
        apply_bridge(
            ServerMessage::Question {
                rpc_id: "rpc".into(),
                session_id: "session".into(),
                questions: vec![crate::protocol::QuestionItem {
                    id: "choice".into(),
                    question: "Choose".into(),
                    header: None,
                    options: Some(vec![crate::protocol::QuestionOption {
                        label: "A".into(),
                        description: None,
                    }]),
                    multi_select: false,
                }],
            },
            &state,
            &mut ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert!(matches!(
            page.as_ref().map(|page| &page.page),
            Some(e_tui::input_page::InputPage::Question(_))
        ));
        assert_eq!(input.buf, "draft prompt");

        let mut config = Config::default();
        let themes = Vec::new();
        let mut theme = config.theme();
        let effects = RuntimeController::apply_input_page_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &state,
            &mut InputPageUiState {
                input_page: &mut page,
                input: &mut input,
                config: &mut config,
                themes: &themes,
                theme: &mut theme,
                question: &mut question,
            },
        );
        assert!(page.is_none());
        assert!(question.is_none());
        assert!(state.lock().unwrap().interaction.question.is_none());
        assert_eq!(input.buf, "draft prompt");
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::AnswerQuestions { request_id, .. })]
                if request_id == "rpc"
        ));
    }

    fn bridge_effects(message: ServerMessage) -> Vec<UiAction> {
        let state = Arc::new(Mutex::new(AppState::default()));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
        apply_bridge(
            message,
            &state,
            &mut ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        )
    }

    fn bridge_error(code: &str, message: &str) -> Vec<UiAction> {
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
            [UiAction::Fatal(reason)] if reason.contains("gone")
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
            [UiAction::Fatal(reason)]
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
            [UiAction::Fatal(reason)]
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
                [UiAction::Fatal(reason)] if reason.contains(expected)
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
            approval_open: true,
            reading_view_open: true,
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
            TerminalRoute::Approval(_)
        ));
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('x')), blocking),
            TerminalRoute::Approval(_)
        ));

        let reading = TerminalFocus {
            approval_open: false,
            ..blocking
        };
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('x')), reading),
            TerminalRoute::Reading(_)
        ));
    }

    #[test]
    fn terminal_router_routes_primary_pointer_and_focus_loss() {
        let focus = TerminalFocus::default();
        let mouse = crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 7,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            route_terminal_event(Event::Mouse(mouse), focus),
            TerminalRoute::Pointer(PointerEvent::PrimaryPress { column: 4, row: 7 })
        );
        assert_eq!(
            route_terminal_event(
                Event::Mouse(crossterm::event::MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column: 4,
                    row: 7,
                    modifiers: KeyModifiers::SHIFT,
                }),
                focus,
            ),
            TerminalRoute::Pointer(PointerEvent::Wheel { up: true })
        );
        assert_eq!(
            route_terminal_event(Event::FocusLost, focus),
            TerminalRoute::Pointer(PointerEvent::FocusLost)
        );
    }

    #[test]
    fn pointer_release_returns_clipboard_effect_without_holding_ui_state() {
        let state = Arc::new(Mutex::new(AppState::default()));
        let mut selection_frame = SelectionFrame::for_viewport(80, 24);
        selection_frame.set_epoch(1);
        selection_frame.push_text(e_tui::SelectionSurface::Transcript, 0, 0, 0, "alpha");
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut input_page = None;
        let mut help_visible = false;
        let mut notice = NoticeState::default();
        let mut mouse_selection = MouseSelection::default();
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut theme = config.theme();
        let mut ui = TerminalUiState {
            scroll: &mut scroll,
            input: &mut input,
            input_page: &mut input_page,
            help_visible: &mut help_visible,
            notice: &mut notice,
            mouse_selection: &mut mouse_selection,
            approval: &mut approval,
            question: &mut question,
            queue: &mut queue,
            config: &mut config,
            themes: &mut themes,
            theme: &mut theme,
        };
        let size = TerminalSize {
            width: 80,
            height: 24,
        };
        assert!(RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryPress { column: 0, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        )
        .is_empty());
        assert!(matches!(
            RuntimeController::apply_terminal_route(
                TerminalRoute::Pointer(PointerEvent::PrimaryRelease { column: 2, row: 0 }),
                size,
                Instant::now(),
                &state,
                &selection_frame,
                &mut ui,
            )
            .as_slice(),
            [UiAction::WriteClipboard(text)] if text == "alp"
        ));

        RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryPress { column: 0, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        );
        RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::FocusLost),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        );
        assert!(RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryRelease { column: 2, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        )
        .is_empty());

        RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryPress { column: 1, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        );
        assert!(RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryRelease { column: 1, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        )
        .is_empty());

        RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryPress { column: 0, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        );
        assert!(RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryRelease { column: 2, row: 0 }),
            TerminalSize {
                width: 81,
                height: 24,
            },
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        )
        .is_empty());
    }

    #[test]
    fn reading_copy_remains_complete_after_partial_mouse_copy() {
        let state = Arc::new(Mutex::new(AppState::default()));
        let source = "complete canonical source";
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        {
            let mut app = state.lock().unwrap();
            app.transcript.append(
                e_tui::display::DisplayItem::Block(e_tui::display::TranscriptBlock {
                    id: e_tui::display::DisplayId::correlated("assistant", "copy-semantics"),
                    unit: Some(1),
                    content: source.into(),
                    format: e_tui::display::TranscriptFormat::Plain,
                    tone: e_tui::display::DisplayTone::Normal,
                    copy_source: source.into(),
                    streaming: false,
                }),
                None,
            );
            assert!(app.enter_reading(&input, &mut scroll, 20));
        }
        let mut selection_frame = SelectionFrame::for_viewport(80, 24);
        selection_frame.set_epoch(1);
        selection_frame.push_text(e_tui::SelectionSurface::Transcript, 0, 0, 0, "partial");
        let mut input_page = None;
        let mut help_visible = false;
        let mut notice = NoticeState::default();
        let mut mouse_selection = MouseSelection::default();
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut theme = config.theme();
        let mut ui = TerminalUiState {
            scroll: &mut scroll,
            input: &mut input,
            input_page: &mut input_page,
            help_visible: &mut help_visible,
            notice: &mut notice,
            mouse_selection: &mut mouse_selection,
            approval: &mut approval,
            question: &mut question,
            queue: &mut queue,
            config: &mut config,
            themes: &mut themes,
            theme: &mut theme,
        };
        let size = TerminalSize {
            width: 80,
            height: 24,
        };
        RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryPress { column: 0, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        );
        let visual = RuntimeController::apply_terminal_route(
            TerminalRoute::Pointer(PointerEvent::PrimaryRelease { column: 2, row: 0 }),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        );
        assert!(matches!(
            visual.as_slice(),
            [UiAction::WriteClipboard(text)] if text == "par"
        ));

        let semantic = RuntimeController::apply_terminal_route(
            TerminalRoute::Reading(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
            size,
            Instant::now(),
            &state,
            &selection_frame,
            &mut ui,
        );
        assert!(matches!(
            semantic.as_slice(),
            [UiAction::WriteClipboard(text)] if text == source
        ));
    }

    #[test]
    fn reading_route_preserves_draft_and_blocks_paste() {
        let reading = TerminalFocus {
            reading_view_open: true,
            ..TerminalFocus::default()
        };
        assert!(matches!(
            route_terminal_event(key(KeyCode::Char('j')), reading),
            TerminalRoute::Reading(_)
        ));
        assert_eq!(
            route_terminal_event(Event::Paste("draft".into()), reading),
            TerminalRoute::Ignore
        );
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
        let mut notice = NoticeState::default();
        let mut mouse_selection = MouseSelection::default();
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut theme = config.theme();

        let command = RuntimeController::apply_terminal_route(
            TerminalRoute::Ordinary(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            TerminalSize {
                width: 120,
                height: 40,
            },
            Instant::now(),
            &state,
            &SelectionFrame::default(),
            &mut TerminalUiState {
                scroll: &mut scroll,
                input: &mut input,
                input_page: &mut input_page,
                help_visible: &mut help_visible,
                notice: &mut notice,
                mouse_selection: &mut mouse_selection,
                approval: &mut approval,
                question: &mut question,
                queue: &mut queue,
                config: &mut config,
                themes: &mut themes,
                theme: &mut theme,
            },
        );
        assert!(matches!(
            command.as_slice(),
            [UiAction::Agent(AgentRequest::Command { line })] if line == "/plugin slow"
        ));
        assert!(state.lock().unwrap().has_active_command());

        let interrupt = RuntimeController::apply_terminal_route(
            TerminalRoute::Ordinary(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            TerminalSize {
                width: 120,
                height: 40,
            },
            Instant::now(),
            &state,
            &SelectionFrame::default(),
            &mut TerminalUiState {
                scroll: &mut scroll,
                input: &mut input,
                input_page: &mut input_page,
                help_visible: &mut help_visible,
                notice: &mut notice,
                mouse_selection: &mut mouse_selection,
                approval: &mut approval,
                question: &mut question,
                queue: &mut queue,
                config: &mut config,
                themes: &mut themes,
                theme: &mut theme,
            },
        );
        assert!(matches!(
            interrupt.as_slice(),
            [UiAction::Agent(AgentRequest::Interrupt)]
        ));

        let before = state.lock().unwrap().transcript.len();
        let mut bridge_ui = ui(
            &mut scroll,
            &mut input,
            &mut input_page,
            &mut approval,
            &mut question,
            &mut queue,
        );
        apply_bridge(
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
        state.lock().unwrap().render.transcript_cache.valid = true;
        RuntimeController::apply_effect_result(
            EffectResult::ClipboardFailed("denied".into()),
            &state,
            Instant::now(),
        );
        let state = state.lock().unwrap();
        assert!(matches!(
            state.msgs.last(),
            Some(Msg::Error { text }) if text.contains("denied")
        ));
        assert!(matches!(
            &state.transcript.nodes().last().unwrap().item,
            e_tui::display::DisplayItem::Block(block)
                if block.tone == e_tui::display::DisplayTone::Error
                    && block.content.contains("denied")
        ));
        assert!(!state.render.transcript_cache.valid);
    }

    #[test]
    fn clipboard_success_uses_frontend_notice_state() {
        let state = Mutex::new(AppState::default());
        let now = Instant::now();
        assert!(RuntimeController::apply_effect_result(
            EffectResult::ClipboardWritten {
                lines: 2,
                preview: "one tw".into(),
                truncated: true,
            },
            &state,
            now,
        ));
        assert_eq!(
            state
                .lock()
                .unwrap()
                .interaction
                .notice
                .visible_text(2, now),
            Some("已复制 2 行：one tw...")
        );
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
        state.lock().unwrap().render.transcript_cache.valid = true;
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut input_page = None;
        let mut help_visible = false;
        let mut notice = NoticeState::default();
        let mut mouse_selection = MouseSelection::default();
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
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
                notice: &mut notice,
                mouse_selection: &mut mouse_selection,
                approval: &mut approval,
                question: &mut question,
                queue: &mut queue,
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
        assert!(!state.render.transcript_cache.valid);
    }

    #[test]
    fn draft_first_prompt_uses_atomic_new_input_without_old_queue() {
        let state = Mutex::new(AppState::default());
        state.lock().unwrap().begin_new_conversation("code");
        let mut queue = Vec::new();
        let outcome = RuntimeController::apply_input_action(
            InputAction::Send("first prompt".into()),
            &state,
            &mut queue,
        );
        assert!(matches!(
            outcome.effects.as_slice(),
            [UiAction::Agent(AgentRequest::NewInput { mode, text })]
                if mode == "code" && text == "first prompt"
        ));
        let state = state.lock().unwrap();
        assert!(state.interaction.queue.is_empty());
        assert_eq!(
            state
                .session
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
        let mut queue = Vec::new();
        let _ = RuntimeController::apply_input_action(
            InputAction::Send("retry me".into()),
            &state,
            &mut queue,
        );
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut input_page = None;
        let mut approval = None;
        let mut question = None;
        let effects = apply_bridge(
            ServerMessage::Error {
                code: "new-failed".into(),
                message: "creation failed".into(),
            },
            &state,
            &mut BridgeUiState {
                scroll: &mut scroll,
                input: &mut input,
                input_page: &mut input_page,
                approval: &mut approval,
                question: &mut question,
                queue: &mut queue,
            },
        );
        assert!(effects.is_empty());
        assert_eq!(input.buf, "retry me");
        assert_eq!(input.cursor, 8);
        let app = state.lock().unwrap();
        assert!(app.is_new_conversation());
        assert!(app
            .session
            .new_conversation
            .as_ref()
            .and_then(|draft| draft.notice.as_deref())
            .is_some_and(|notice| notice.contains("creation failed")));
    }

    #[test]
    fn queued_dispatch_returns_send_after_atomic_state_change() {
        let state = Mutex::new(AppState::default());
        state.lock().unwrap().interaction.queue.push("next".into());
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(state.lock().unwrap().interaction.queue.is_empty());
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { text })] if text == "next"
        ));
    }

    /// Regression: a Send while the agent runs must survive the main loop's
    /// `take(&mut app.interaction)` + restore round trip (6cdb025b moved the
    /// queue into the InteractionModel; handlers used to push into a transient
    /// default that the restore discarded, silently eating the prompt).
    #[test]
    fn send_while_running_queues_into_the_live_interaction() {
        let state = Arc::new(Mutex::new(AppState::default()));
        state.lock().unwrap().session.status = crate::model::AgentStatus::Running;
        let mut interaction = {
            let mut app = state.lock().unwrap();
            std::mem::take(&mut app.interaction)
        };
        let outcome = RuntimeController::apply_input_action(
            InputAction::Send("排队测试".into()),
            &state,
            &mut interaction.queue,
        );
        assert!(
            outcome.effects.is_empty(),
            "running must not send immediately"
        );
        state.lock().unwrap().interaction = interaction;
        assert_eq!(state.lock().unwrap().interaction.queue, vec!["排队测试"]);

        // The idle transition then auto-dispatches the queued prompt.
        {
            let mut app = state.lock().unwrap();
            app.session.status = crate::model::AgentStatus::Idle;
            app.session.working = false;
        }
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { text })] if text == "排队测试"
        ));
        assert!(state.lock().unwrap().interaction.queue.is_empty());
    }

    /// Regression: an approval frame must reach the live card (not a transient
    /// default) and the next key press must answer it.
    #[test]
    fn approval_card_survives_take_restore_and_answers() {
        let state = Arc::new(Mutex::new(AppState::default()));
        {
            let mut interaction = {
                let mut app = state.lock().unwrap();
                std::mem::take(&mut app.interaction)
            };
            {
                let mut ui = BridgeUiState {
                    scroll: &mut interaction.scroll,
                    input: &mut interaction.input,
                    input_page: &mut interaction.input_page,
                    approval: &mut interaction.approval,
                    question: &mut interaction.question,
                    queue: &mut interaction.queue,
                };
                apply_bridge(
                    ServerMessage::Approval {
                        id: "a1".into(),
                        tool_name: "bash".into(),
                        reason: "run".into(),
                        call_id: None,
                    },
                    &state,
                    &mut ui,
                );
            }
            state.lock().unwrap().interaction = interaction;
        }
        assert!(state.lock().unwrap().interaction.approval.is_some());

        let mut interaction = {
            let mut app = state.lock().unwrap();
            std::mem::take(&mut app.interaction)
        };
        let effects = RuntimeController::answer_approval(
            &KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut interaction.approval,
        );
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::ApprovalAnswer { id, allow })]
                if id == "a1" && *allow
        ));
        state.lock().unwrap().interaction = interaction;
        assert!(state.lock().unwrap().interaction.approval.is_none());
    }
}
