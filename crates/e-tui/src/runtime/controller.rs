//! Testable runtime inputs, effects, and bridge-message controller.
//!
//! The Tokio loop in `main.rs` owns waiting and concrete I/O. This module
//! mutates typed client state and returns effects that the runner executes only
//! after state guards have been released.

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::{
    command_catalog::NewMode,
    input::{InputAction, InputState},
    input_page::{InputPageSession, PageEffect},
    interaction::ApprovalCard,
    login::LoginView,
    question::QuestionBatch,
    runtime::{
        command::{self as runtime_command, LocalCommandContext},
        input::TerminalRoute,
        state::RuntimeState,
    },
    theme,
    ui::{scroll_lines, scroll_page, transcript_view_height, ScrollState, TerminalSize},
    AgentEvent, AgentRequest, ClipboardPaste, Config, MouseSelection, NoticeState, PaneResizeState,
    PointerEvent, PromptImage, PromptInput, SelectionFrame, Theme, ThemeFile,
};
pub use crate::{DrawPriority, EffectResult, UiAction};

pub enum RuntimeInput {
    Bridge(AgentEvent),
    Terminal(Event),
    EffectCompleted(EffectResult),
    AnimationDeadline,
    FrameDeadline,
}

/// UI-local page transitions consumed synchronously by the controller.
pub enum ControllerAction {
    OpenPage(InputPageSession),
    ClosePage,
}

fn paste_text(
    input: &mut InputState,
    input_page: &mut Option<InputPageSession>,
    text: &str,
) -> bool {
    if text.is_empty() {
        return false;
    }
    if let Some(page) = input_page {
        page.paste(text)
    } else {
        input.paste(text);
        true
    }
}

fn agent_action(request: AgentRequest) -> UiAction {
    UiAction::Agent(request)
}

/// Mutable UI-local state affected by bridge frames. Keeping it separate from
/// RuntimeState makes session-switch behavior explicit without giving the bridge
/// handler ownership of terminal or transport infrastructure. The interaction
/// fields (approval/question/queue) are borrowed from the same InteractionModel
/// the terminal path uses, so bridge frames and key handling mutate the same
/// object — never a transient default.
pub struct RuntimeUiState<'a> {
    pub scroll: &'a mut ScrollState,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
    pub approval: &'a mut Option<ApprovalCard>,
    pub question: &'a mut Option<String>,
    pub queue: &'a mut Vec<PromptInput>,
}

#[derive(Default)]
pub struct InputHandlerOutcome {
    pub command: Option<PendingCommand>,
    pub activate_reading: bool,
    pub effects: Vec<UiAction>,
}

pub struct PendingCommand {
    pub line: String,
    pub images: Vec<PromptImage>,
    pub original: PromptInput,
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
    pub pane_resize: &'a mut PaneResizeState,
    pub approval: &'a mut Option<ApprovalCard>,
    pub question: &'a mut Option<String>,
    pub queue: &'a mut Vec<PromptInput>,
    pub config: &'a mut Config,
    pub themes: &'a mut Vec<ThemeFile>,
    pub theme: &'a mut Theme,
}

pub struct RuntimeController;

fn normalized_session_status(status: &crate::agent::AgentStatus) -> crate::SessionStatus {
    match status {
        crate::agent::AgentStatus::Running | crate::agent::AgentStatus::Waiting => {
            crate::SessionStatus::Running
        }
        crate::agent::AgentStatus::Idle
        | crate::agent::AgentStatus::Error
        | crate::agent::AgentStatus::Custom(_) => crate::SessionStatus::Idle,
    }
}

impl RuntimeController {
    pub fn apply_terminal_route(
        route: TerminalRoute,
        size: TerminalSize,
        now: Instant,
        state: &Arc<Mutex<RuntimeState>>,
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
                    effects.push(agent_action(AgentRequest::History {
                        before_sequence: before_seq,
                        limit: 400,
                    }));
                }
            }
            TerminalRoute::Pointer(pointer) => {
                let area = Rect::new(0, 0, size.width, size.height);
                let preview_fullscreen = state.lock().unwrap().preview.fullscreen;
                let separator_hit = matches!(
                    pointer,
                    PointerEvent::PrimaryPress { column, .. }
                        if crate::ui::screen::separator_hit(
                            area,
                            ui.config.message_pane_percent,
                            preview_fullscreen,
                            column,
                        )
                );

                if separator_hit {
                    ui.mouse_selection.clear();
                    let collapsed = matches!(
                        crate::ui::screen::layout(
                            area,
                            ui.config.message_pane_percent,
                            preview_fullscreen,
                        ),
                        crate::ui::screen::ScreenLayout::MainOnly(_)
                    );
                    ui.pane_resize.begin(
                        match pointer {
                            PointerEvent::PrimaryPress { column, .. } => column,
                            _ => unreachable!("separator hit only matches primary press"),
                        },
                        ui.config.message_pane_percent,
                        collapsed,
                    );
                    return effects;
                }

                if ui.pane_resize.is_active() {
                    match pointer {
                        PointerEvent::PrimaryDrag { column, .. } => {
                            ui.pane_resize.update(column, size.width);
                            ui.mouse_selection.clear();
                        }
                        PointerEvent::PrimaryRelease { .. } => {
                            if let Some(drag) = ui.pane_resize.finish() {
                                ui.config.message_pane_percent = drag.pending_percent;
                                state.lock().unwrap().config.message_pane_percent =
                                    drag.pending_percent;
                                effects.push(UiAction::PersistConfig(ui.config.clone()));
                            }
                            ui.mouse_selection.clear();
                        }
                        PointerEvent::FocusLost => {
                            ui.pane_resize.cancel();
                            ui.mouse_selection.clear();
                        }
                        _ => {}
                    }
                    return effects;
                }

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
                paste_text(ui.input, ui.input_page, &text);
            }
            TerminalRoute::ReadClipboard => effects.push(UiAction::ReadClipboard),
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
        state: &Arc<Mutex<RuntimeState>>,
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
                        crate::ReadingDirection::Down,
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
                    app.move_reading_item(crate::ReadingDirection::Up, ui.scroll, viewport_height);
                } else {
                    app.move_reading_block(-1, ui.scroll, viewport_height);
                }
                app.take_actions()
            }
            KeyCode::Left | KeyCode::Char('h') if key.modifiers.is_empty() && item_mode => {
                let mut app = state.lock().unwrap();
                app.move_reading_item(crate::ReadingDirection::Left, ui.scroll, viewport_height);
                app.take_actions()
            }
            KeyCode::Right | KeyCode::Char('l') if key.modifiers.is_empty() => {
                let mut app = state.lock().unwrap();
                if item_mode {
                    app.move_reading_item(
                        crate::ReadingDirection::Right,
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
        state: &Arc<Mutex<RuntimeState>>,
        ui: &mut TerminalUiState<'_>,
    ) -> Vec<UiAction> {
        if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
            Self::apply_action(
                ControllerAction::OpenPage(InputPageSession::resume()),
                ui.input_page,
            );
            return vec![agent_action(AgentRequest::ListSessions)];
        }

        let (idle, catalogs) = {
            let app = state.lock().unwrap();
            (
                app.is_new_conversation()
                    || (app.session.status == crate::SessionStatus::Idle
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
        if let Some(pending) = outcome.command {
            let command_name = pending
                .line
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_start_matches('/');
            let is_builtin = crate::command_catalog::BUILTIN_COMMANDS
                .iter()
                .any(|command| command.name == command_name);
            let is_skill_injection = command_name.eq_ignore_ascii_case("skill")
                || command_name.split_once(':').is_some_and(|(prefix, skill)| {
                    prefix.eq_ignore_ascii_case("skill") && !skill.is_empty()
                });
            if (is_builtin || is_skill_injection) && !pending.images.is_empty() {
                ui.input.restore_prompt(pending.original);
                ui.notice.show("此命令不接受图片", now);
                return outcome.effects;
            }
            let command = runtime_command::handle_local_command(
                pending.line,
                LocalCommandContext {
                    input_page: ui.input_page,
                    integrated_commands: &catalogs.integrated_commands,
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
                .extend(command.outbound.into_iter().map(|request| {
                    let request = match request {
                        AgentRequest::Command { line, .. } => AgentRequest::Command {
                            line,
                            images: pending.images.clone(),
                        },
                        other => other,
                    };
                    agent_action(request)
                }));
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
    pub fn apply_agent(
        event: AgentEvent,
        state: &Arc<Mutex<RuntimeState>>,
        ui: &mut RuntimeUiState<'_>,
    ) -> Vec<UiAction> {
        use crate::agent::{InteractionEvent, TimelineEvent};

        match event {
            AgentEvent::Timeline(TimelineEvent::Snapshot { records, truncated }) => {
                let _zone = crate::tracy_zone!("snapshot apply");
                let mut app = state.lock().unwrap();
                app.apply_snapshot(&records, truncated);
                app.take_actions()
            }
            AgentEvent::Timeline(TimelineEvent::Append(record)) => {
                let mut app = state.lock().unwrap();
                let commits_new_conversation = matches!(
                    &record.fact,
                    crate::agent::timeline::TimelineFact::UserMessage {
                        source_kind: Some(kind),
                        ..
                    } if kind == "user"
                );
                if commits_new_conversation {
                    app.session.new_conversation = None;
                }
                app.apply_host_event(&record);
                app.take_actions()
            }
            AgentEvent::Timeline(TimelineEvent::History { records, has_more }) => {
                let mut app = state.lock().unwrap();
                app.prepend_host_events(&records);
                app.session.history_loading = false;
                app.session.history_exhausted = !has_more;
                app.take_actions()
            }
            AgentEvent::Session(event) => Self::apply_session(event, state, ui),
            AgentEvent::Catalog(event) => Self::apply_catalog(event, state, ui),
            AgentEvent::Interaction(InteractionEvent::SetEditorText { text }) => {
                if ui.input_page.is_none() {
                    ui.input.restore_text(text);
                    vec![UiAction::RequestDraw(DrawPriority::Interactive)]
                } else {
                    Vec::new()
                }
            }
            AgentEvent::Interaction(event) => Self::apply_interaction(event, state, ui),
            AgentEvent::Preview(crate::agent::PreviewEvent::Resolved {
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
                visible
                    .then(|| UiAction::RequestDraw(DrawPriority::Content))
                    .into_iter()
                    .collect()
            }
            AgentEvent::EffectCompleted(result) => {
                let dirty = Self::apply_effect_result(result, state, Instant::now());
                dirty
                    .then(|| UiAction::RequestDraw(DrawPriority::Content))
                    .into_iter()
                    .collect()
            }
            AgentEvent::Deadline(_) => Vec::new(),
        }
    }

    fn apply_session(
        event: crate::agent::SessionEvent,
        state: &Arc<Mutex<RuntimeState>>,
        ui: &mut RuntimeUiState<'_>,
    ) -> Vec<UiAction> {
        use crate::agent::SessionEvent;

        match event {
            SessionEvent::Attached(attached) => {
                let switched = {
                    let mut app = state.lock().unwrap();
                    let switched = app.session.session_id.as_deref() != Some(attached.id.as_str());
                    if switched {
                        app.reset_transcript();
                        app.interaction.question = None;
                    }
                    app.session.session_id = Some(attached.id.clone());
                    let materialization_pending = app
                        .session
                        .new_conversation
                        .as_ref()
                        .is_some_and(|draft| draft.pending_input.is_some());
                    if !materialization_pending {
                        app.session.new_conversation = None;
                    }
                    app.session.session_title = attached.title;
                    app.session.session_cwd = attached.workspace;
                    app.session.status = normalized_session_status(&attached.status);
                    app.session.working = app.session.status == crate::SessionStatus::Running;
                    app.session.provider = attached.provider;
                    app.session.model = attached.model;
                    app.session.current_mode = attached
                        .mode
                        .or_else(|| Some(app.config.default_mode.clone()));
                    app.session.current_mode_seq = None;
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
                vec![UiAction::PersistSessionId(attached.id)]
            }
            SessionEvent::Status(status) => {
                let mut app = state.lock().unwrap();
                app.session.status = normalized_session_status(&status);
                if app.session.status == crate::SessionStatus::Idle {
                    app.stop_thinking();
                }
                Vec::new()
            }
            SessionEvent::Title(title) => {
                state.lock().unwrap().session.session_title = Some(title);
                Vec::new()
            }
            SessionEvent::List {
                sessions,
                titles_pending,
            } => {
                state.lock().unwrap().catalogs.sessions = sessions.clone();
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_sessions(sessions, titles_pending);
                }
                Vec::new()
            }
        }
    }

    fn apply_catalog(
        event: crate::agent::CatalogEvent,
        state: &Arc<Mutex<RuntimeState>>,
        ui: &mut RuntimeUiState<'_>,
    ) -> Vec<UiAction> {
        use crate::agent::CatalogEvent;

        match event {
            CatalogEvent::Presets(presets) => {
                let modes = presets
                    .into_iter()
                    .filter(|preset| preset.unavailable_reason.is_none())
                    .map(|preset| NewMode {
                        id: preset.id,
                        name: preset.name,
                        description: preset.description,
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
            }
            CatalogEvent::Skills(skills) => {
                let catalogs = {
                    let mut app = state.lock().unwrap();
                    app.catalogs.skills = skills;
                    app.catalogs.clone()
                };
                ui.input.catalog_changed(&catalogs);
            }
            CatalogEvent::Commands(commands) => {
                let catalogs = {
                    let mut app = state.lock().unwrap();
                    app.catalogs.integrated_commands = commands;
                    app.catalogs.clone()
                };
                ui.input.catalog_changed(&catalogs);
            }
            CatalogEvent::Login {
                providers,
                proxies,
                error,
            } => {
                {
                    let mut app = state.lock().unwrap();
                    app.catalogs.credential_providers = providers.clone();
                    app.catalogs.proxies = proxies.clone();
                }
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_login(LoginView {
                        providers,
                        proxies,
                        error,
                    });
                }
            }
            CatalogEvent::Models { providers, current } => {
                let selected = current
                    .as_ref()
                    .map(|current| (current.provider.clone(), current.model.clone()));
                let mut app = state.lock().unwrap();
                app.catalogs.model_providers = providers.clone();
                app.catalogs.current_model = current.clone();
                let catalogs = app.catalogs.clone();
                if let Some(current) = current {
                    app.session.provider = Some(current.provider);
                    app.session.model = Some(current.model);
                }
                drop(app);
                if let Some(page) = ui.input_page.as_mut() {
                    page.apply_model(providers, selected);
                    page.apply_effort(&catalogs);
                }
            }
        }
        Vec::new()
    }

    fn apply_interaction(
        event: crate::agent::InteractionEvent,
        state: &Arc<Mutex<RuntimeState>>,
        ui: &mut RuntimeUiState<'_>,
    ) -> Vec<UiAction> {
        use crate::agent::InteractionEvent;

        match event {
            InteractionEvent::CommandResult { id, outcome, text } => {
                let mut app = state.lock().unwrap();
                app.finish_command_execution();
                app.apply_command_result(&id, &outcome, text.as_deref());
            }
            InteractionEvent::Approval {
                id, label, reason, ..
            } => {
                *ui.approval = Some(ApprovalCard {
                    id,
                    tool_name: label,
                    reason,
                });
            }
            InteractionEvent::Question {
                request_id,
                session_id,
                questions,
            } => {
                *ui.question = Some(request_id.clone());
                *ui.input_page = Some(InputPageSession::question(QuestionBatch::new(
                    request_id, session_id, questions,
                )));
            }
            InteractionEvent::QuestionResolved { request_id, .. } => {
                if ui.question.as_deref() == Some(request_id.as_str()) {
                    *ui.question = None;
                }
                if ui
                    .input_page
                    .as_ref()
                    .and_then(InputPageSession::question_rpc_id)
                    == Some(request_id.as_str())
                {
                    *ui.input_page = None;
                }
            }
            InteractionEvent::Error { code, message } => {
                return Self::apply_agent_error(&code, &message, state, ui);
            }
            InteractionEvent::Heartbeat | InteractionEvent::SetEditorText { .. } => {}
        }
        Vec::new()
    }

    fn apply_agent_error(
        code: &str,
        message: &str,
        state: &Arc<Mutex<RuntimeState>>,
        ui: &mut RuntimeUiState<'_>,
    ) -> Vec<UiAction> {
        if code == "fatal" {
            return vec![UiAction::Fatal(message.to_owned())];
        }
        if matches!(
            code,
            "new-failed" | "new-input-failed" | "image-input-too-large"
        ) && state.lock().unwrap().is_new_conversation()
        {
            let restored = {
                let mut app = state.lock().unwrap();
                let restored = app.restore_new_conversation_input();
                if restored.is_some() {
                    app.set_new_conversation_notice(format!("创建新对话失败：{message}"));
                }
                restored
            };
            if let Some(prompt) = restored {
                ui.input.restore_prompt(prompt);
                ui.input.multiline = ui.input.buf.contains('\n');
            } else {
                state
                    .lock()
                    .unwrap()
                    .push_error_message(format!("运行时错误 {code}: {message}"));
            }
            return Vec::new();
        }
        if code == "command-cancelled" {
            state.lock().unwrap().finish_command_execution();
            return Vec::new();
        }
        let mut app = state.lock().unwrap();
        if matches!(
            code,
            "no-commands" | "command-unknown" | "command-invalid-result" | "command-failed"
        ) {
            app.finish_command_execution();
        }
        app.push_error_message(format!("运行时错误 {code}: {message}"));
        Vec::new()
    }

    pub fn apply_action(action: ControllerAction, input_page: &mut Option<InputPageSession>) {
        match action {
            ControllerAction::OpenPage(page) => *input_page = Some(page),
            ControllerAction::ClosePage => *input_page = None,
        }
    }

    pub fn apply_input_action(
        action: InputAction,
        state: &Mutex<RuntimeState>,
        queue: &mut Vec<PromptInput>,
    ) -> InputHandlerOutcome {
        let mut outcome = InputHandlerOutcome::default();
        match action {
            InputAction::None | InputAction::ToggleMultiline => {}
            InputAction::Send(prompt) => {
                let new_input = {
                    let mut state = state.lock().unwrap();
                    if state.is_new_conversation() {
                        state.materialize_new_conversation(prompt.clone())
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
                    let immediate = state.enqueue_or_immediate(&prompt, queue);
                    if immediate {
                        state.start_thinking();
                    }
                    immediate
                };
                if immediate {
                    outcome
                        .effects
                        .push(agent_action(AgentRequest::Input { prompt }));
                }
            }
            InputAction::Command {
                line,
                images,
                original,
            } => {
                outcome.command = Some(PendingCommand {
                    line,
                    images,
                    original,
                })
            }
            InputAction::Interrupt => {
                queue.clear();
                outcome.effects.push(agent_action(AgentRequest::Interrupt));
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
        state: &Mutex<RuntimeState>,
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
        state: &Mutex<RuntimeState>,
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
        state: &Mutex<RuntimeState>,
        now: Instant,
    ) -> bool {
        match result {
            EffectResult::ConfigPersisted(Ok(())) | EffectResult::ConfigReloaded { .. } => false,
            EffectResult::ClipboardRead(Ok(content)) => {
                let mut app = state.lock().unwrap();
                let interaction = &mut app.interaction;
                match content {
                    ClipboardPaste::Text(text) => {
                        let text = crate::input::normalize_paste_text(&text);
                        paste_text(&mut interaction.input, &mut interaction.input_page, &text)
                    }
                    ClipboardPaste::Image(image) => {
                        if interaction.input_page.is_some() {
                            false
                        } else {
                            interaction.input.paste_image(image);
                            true
                        }
                    }
                }
            }
            EffectResult::ClipboardRead(Err(error)) => {
                state
                    .lock()
                    .unwrap()
                    .push_error_message(format!("剪贴板读取失败: {error}"));
                true
            }
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
    pub fn dispatch_next_queued(state: &Mutex<RuntimeState>) -> Vec<UiAction> {
        let mut state = state.lock().unwrap();
        let Some(prompt) = state.take_next_queued() else {
            return Vec::new();
        };
        state.start_thinking();
        vec![agent_action(AgentRequest::Input { prompt })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent::InteractionEvent, input::InputState};

    fn runtime_ui<'a>(
        scroll: &'a mut ScrollState,
        input: &'a mut InputState,
        input_page: &'a mut Option<InputPageSession>,
        approval: &'a mut Option<ApprovalCard>,
        question: &'a mut Option<String>,
        queue: &'a mut Vec<PromptInput>,
    ) -> RuntimeUiState<'a> {
        RuntimeUiState {
            scroll,
            input,
            input_page,
            approval,
            question,
            queue,
        }
    }

    #[test]
    fn external_editor_text_only_targets_the_visible_composer() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
        let effects = RuntimeController::apply_agent(
            AgentEvent::Interaction(InteractionEvent::SetEditorText {
                text: "visible draft".into(),
            }),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert_eq!(input.buf, "visible draft");
        assert!(matches!(
            effects.as_slice(),
            [UiAction::RequestDraw(DrawPriority::Interactive)]
        ));

        page = Some(InputPageSession::login());
        RuntimeController::apply_agent(
            AgentEvent::Interaction(InteractionEvent::SetEditorText {
                text: "hidden".into(),
            }),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert_eq!(input.buf, "visible draft");
    }

    #[test]
    fn clipboard_image_completion_targets_only_the_visible_composer() {
        let state = Mutex::new(RuntimeState::default());
        let image = PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some("clip.png".into()),
        };
        assert!(RuntimeController::apply_effect_result(
            EffectResult::ClipboardRead(Ok(ClipboardPaste::Image(image.clone()))),
            &state,
            Instant::now(),
        ));
        assert_eq!(
            state.lock().unwrap().interaction.input.display_text().text,
            "[Image clip.png]"
        );

        state.lock().unwrap().interaction.input_page = Some(InputPageSession::login());
        assert!(!RuntimeController::apply_effect_result(
            EffectResult::ClipboardRead(Ok(ClipboardPaste::Image(image))),
            &state,
            Instant::now(),
        ));
        assert_eq!(
            state.lock().unwrap().interaction.input.display_text().text,
            "[Image clip.png]"
        );
    }

    #[test]
    fn queued_prompt_is_claimed_atomically_when_idle() {
        let state = Mutex::new(RuntimeState::default());
        state.lock().unwrap().interaction.queue.push("next".into());
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(state.lock().unwrap().interaction.queue.is_empty());
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { prompt })] if prompt.plain_text() == Some("next")
        ));
    }

    #[test]
    fn image_prompt_queues_and_dispatches_without_losing_bytes() {
        let state = Mutex::new(RuntimeState::default());
        state.lock().unwrap().session.status = crate::SessionStatus::Running;
        let image = PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some("clip.png".into()),
        };
        let prompt = PromptInput {
            parts: vec![crate::PromptPart::Image(image.clone())],
        };
        let mut queue = Vec::new();
        let outcome = RuntimeController::apply_input_action(
            InputAction::Send(prompt.clone()),
            &state,
            &mut queue,
        );
        assert!(outcome.effects.is_empty());
        assert_eq!(queue, [prompt.clone()]);

        {
            let mut app = state.lock().unwrap();
            app.session.status = crate::SessionStatus::Idle;
            app.session.working = false;
            app.interaction.queue = queue;
        }
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { prompt: sent })] if sent == &prompt
        ));
    }

    #[test]
    fn failed_image_draft_materialization_restores_the_atomic_block() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        state.lock().unwrap().begin_new_conversation("standard");
        let image = PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some("clip.png".into()),
        };
        let prompt = PromptInput {
            parts: vec![crate::PromptPart::Image(image)],
        };
        let mut queue = Vec::new();
        let outcome = RuntimeController::apply_input_action(
            InputAction::Send(prompt.clone()),
            state.as_ref(),
            &mut queue,
        );
        assert!(matches!(
            outcome.effects.as_slice(),
            [UiAction::Agent(AgentRequest::NewInput { prompt: sent, .. })] if sent == &prompt
        ));

        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        RuntimeController::apply_agent(
            AgentEvent::Session(crate::agent::SessionEvent::Attached(
                crate::agent::AttachedSession {
                    protocol_version: Some(7),
                    max_frame_bytes: None,
                    id: "new-session".into(),
                    status: crate::agent::AgentStatus::Idle,
                    provider: None,
                    model: None,
                    mode: Some("standard".into()),
                    title: None,
                    workspace: None,
                },
            )),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert!(state.lock().unwrap().is_new_conversation());

        RuntimeController::apply_agent_error(
            "new-input-failed",
            "bad image",
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert_eq!(input.display_text().text, "[Image clip.png]");
    }

    #[test]
    fn first_direct_user_message_commits_the_attached_new_conversation() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        state.lock().unwrap().begin_new_conversation("standard");
        let _ = state
            .lock()
            .unwrap()
            .materialize_new_conversation(PromptInput::text("first"));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
        RuntimeController::apply_agent(
            AgentEvent::Timeline(crate::agent::TimelineEvent::Append(
                crate::agent::TimelineRecord {
                    sequence: Some(1),
                    time_ms: None,
                    surface: Some(crate::agent::SurfaceOperation::Append),
                    source_sequences: Vec::new(),
                    fact: crate::agent::TimelineFact::UserMessage {
                        text: "first".into(),
                        source_kind: Some("user".into()),
                        content: vec![crate::agent::ContentBlock::Text("first".into())],
                        source: Default::default(),
                    },
                },
            )),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert!(!state.lock().unwrap().is_new_conversation());
    }

    #[test]
    fn clipboard_read_failure_becomes_visible_without_reentrant_locking() {
        let state = Mutex::new(RuntimeState::default());
        assert!(RuntimeController::apply_effect_result(
            EffectResult::ClipboardRead(Err("denied".into())),
            &state,
            Instant::now(),
        ));
        assert!(!state.lock().unwrap().transcript.is_empty());
    }
}
