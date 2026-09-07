//! Terminal, pointer, selection, Reading, and ordinary-key controller behavior.

use super::{
    agent_action, paste_text, runtime_command, scroll_lines, scroll_page, transcript_view_height,
    AgentRequest, Arc, InputPageSession, InputPageUiState, Instant, KeyEvent, LocalCommandContext,
    Mutex, PointerEvent, Rect, RuntimeState, ScrollState, SelectionFrame, TerminalRoute,
    TerminalSize, TerminalUiState, UiAction,
};
use crate::i18n::tr;
use crate::key_mapping::{Action, Scope};

pub(super) fn apply_terminal_route(
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
                    ui.queue.entries(),
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
                    if let Some(seq) = app.session.min_seq {
                        app.session.history_loading = true;
                        Some(seq)
                    } else {
                        None
                    }
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
        TerminalRoute::Global(action) => {
            ui.mouse_selection.clear();
            match action {
                Action::EnterReadMode => effects.extend(enter_reading(size, now, state, ui)),
                Action::TogglePreview => {
                    let mut app = state.lock().unwrap();
                    app.preview.fullscreen = !app.preview.fullscreen;
                }
                Action::ChooseModel => {
                    *ui.input_page = Some(InputPageSession::model());
                    effects.push(agent_action(AgentRequest::ModelGet));
                }
                Action::ChooseEffort => {
                    *ui.input_page = Some(InputPageSession::effort());
                    effects.push(agent_action(AgentRequest::ModelGet));
                }
                Action::ResumeSession => {
                    *ui.input_page = Some(InputPageSession::resume());
                    effects.push(agent_action(AgentRequest::ListSessions));
                }
                Action::OpenSettings => {
                    *ui.input_page =
                        Some(InputPageSession::settings(crate::settings::SettingsState {
                            modes: state
                                .lock()
                                .unwrap()
                                .catalogs
                                .new_modes
                                .iter()
                                .map(|m| m.id.clone())
                                .collect(),
                            themes: ui.themes.iter().map(|t| t.name.clone()).collect(),
                            ..Default::default()
                        }));
                }
                _ => {}
            }
        }
        TerminalRoute::InputPage(key) => {
            ui.mouse_selection.clear();
            effects.extend(super::input::apply_input_page_key(
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
            effects.extend(super::input::answer_approval(
                ui.config.key_mapping.resolve(Scope::Approval, &key),
                ui.approval,
            ))
        }
        TerminalRoute::Reading(key) => {
            ui.mouse_selection.clear();
            effects.extend(apply_reading_key(&key, size, state, ui));
        }
        TerminalRoute::Ordinary(key) => {
            ui.mouse_selection.clear();
            effects.extend(apply_ordinary_key(key, size, now, state, ui));
        }
        TerminalRoute::Ignore => {}
    }
    effects
}

fn enter_reading(
    size: TerminalSize,
    now: Instant,
    state: &Arc<Mutex<RuntimeState>>,
    ui: &mut TerminalUiState<'_>,
) -> Vec<UiAction> {
    let viewport_height = {
        let app = state.lock().unwrap();
        transcript_view_height(
            size,
            &app,
            ui.input,
            false,
            ui.approval.as_ref(),
            ui.queue.entries(),
        )
    };
    let (entered, actions) = {
        let mut app = state.lock().unwrap();
        let entered = app.enter_reading(ui.input, ui.scroll, viewport_height);
        (entered, app.take_actions())
    };
    if !entered {
        ui.notice
            .show(tr(ui.config.language, "terminal.no_readable_content"), now);
    }
    actions
}

pub(super) fn apply_reading_key(
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
            ui.queue.entries(),
        )
    };
    let item_mode = state
        .lock()
        .unwrap()
        .reading
        .as_ref()
        .is_some_and(|reading| reading.item_cursor.is_some());
    let scope = if item_mode {
        Scope::ReadModeItem
    } else {
        Scope::ReadMode
    };
    match ui.config.key_mapping.resolve(scope, key) {
        Some(Action::Exit) => {
            let mut app = state.lock().unwrap();
            app.exit_reading(ui.input);
            app.take_actions()
        }
        Some(Action::BackToBlocks) => {
            let mut app = state.lock().unwrap();
            app.leave_reading_items();
            app.take_actions()
        }
        Some(
            action
            @ (Action::MoveDown | Action::MoveUp | Action::MoveDownFast | Action::MoveUpFast),
        ) => {
            let up = matches!(action, Action::MoveUp | Action::MoveUpFast);
            let steps = if matches!(action, Action::MoveUpFast | Action::MoveDownFast) {
                15
            } else {
                1
            };
            let mut app = state.lock().unwrap();
            for _ in 0..steps {
                let in_items = app
                    .reading
                    .as_ref()
                    .is_some_and(|reading| reading.item_cursor.is_some());
                let moved = if in_items {
                    let direction = if up {
                        crate::ReadingDirection::Up
                    } else {
                        crate::ReadingDirection::Down
                    };
                    app.move_reading_item(direction, ui.scroll, viewport_height)
                } else {
                    app.move_reading_block(if up { -1 } else { 1 }, ui.scroll, viewport_height)
                };
                if !moved {
                    break;
                }
            }
            app.take_actions()
        }
        Some(Action::MoveLeft) if item_mode => {
            let mut app = state.lock().unwrap();
            app.move_reading_item(crate::ReadingDirection::Left, ui.scroll, viewport_height);
            app.take_actions()
        }
        Some(Action::MoveRight | Action::EnterItems) => {
            let mut app = state.lock().unwrap();
            if item_mode {
                app.move_reading_item(crate::ReadingDirection::Right, ui.scroll, viewport_height);
            } else {
                app.enter_reading_items();
            }
            app.take_actions()
        }
        Some(Action::CopyBlock) => state
            .lock()
            .unwrap()
            .reading_copy_text()
            .map(|text| vec![UiAction::WriteClipboard(text)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub(super) fn apply_ordinary_key(
    key: KeyEvent,
    size: TerminalSize,
    now: Instant,
    state: &Arc<Mutex<RuntimeState>>,
    ui: &mut TerminalUiState<'_>,
) -> Vec<UiAction> {
    ui.input.key_mapping = ui.config.key_mapping.clone();
    if ui
        .config
        .key_mapping
        .resolve(ui.input.key_scope(false), &key)
        == Some(Action::CancelOrInterrupt)
        && !ui.queue.is_empty()
    {
        if ui.queue.has_asap() {
            ui.queue.cancel_asap();
            return ui
                .queue
                .take_clear_request()
                .then_some(UiAction::Agent(crate::AgentRequest::ClearAsap))
                .into_iter()
                .collect();
        }
        ui.queue.cancel_latest();
        return Vec::new();
    }

    let (idle, catalogs) = {
        let app = state.lock().unwrap();
        (
            app.is_new_conversation()
                || (app.session.status == crate::SessionStatus::Idle && !app.has_active_command()),
            app.catalogs.clone(),
        )
    };
    let action = ui.input.handle_key_with_catalog(&key, idle, &catalogs);
    if matches!(
        action,
        super::InputAction::Send(_) | super::InputAction::SendAfterTurn(_)
    ) {
        *ui.scroll = ScrollState::default();
        ui.mouse_selection.clear();
    }
    let mut outcome = super::input::apply_input_action(action, state, ui.queue);
    if outcome.activate_reading {
        outcome.effects.extend(enter_reading(size, now, state, ui));
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
            ui.notice
                .show(tr(ui.config.language, "terminal.command_no_images"), now);
            return outcome.effects;
        }
        let command = runtime_command::handle_local_command(
            pending.line,
            LocalCommandContext {
                language: ui.config.language,
                input_page: ui.input_page,
                integrated_commands: &catalogs.integrated_commands,
                config: ui.config,
                themes: ui.themes,
                new_modes: &catalogs.new_modes,
                model_providers: &catalogs.model_providers,
                current_model: catalogs.current_model.as_ref(),
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
        if is_skill_injection && !command.outbound.is_empty() {
            *ui.scroll = ScrollState::default();
            ui.mouse_selection.clear();
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
                    ui.queue.entries(),
                )
            };
            let mut app = state.lock().unwrap();
            if !app.enter_reading(ui.input, ui.scroll, viewport_height) {
                ui.notice
                    .show(tr(ui.config.language, "terminal.no_readable_content"), now);
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
