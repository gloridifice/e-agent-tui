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
    {
        let mut app = state.lock().unwrap();
        let armed = app.link_copy.armed;
        if !matches!(route, TerminalRoute::Ignore) {
            app.link_copy.armed = false;
        }
        if armed && app.session.new_conversation.is_none() {
            if let TerminalRoute::Ordinary(key) = &route {
                if key.modifiers.is_empty() {
                    if let crossterm::event::KeyCode::Char(tag) = key.code {
                        return app
                            .link_copy
                            .target(tag)
                            .map(UiAction::WriteClipboard)
                            .into_iter()
                            .collect();
                    }
                }
                return Vec::new();
            }
        }
    }
    if *ui.help_visible {
        if let TerminalRoute::Pointer(PointerEvent::Wheel { up, .. }) = &route {
            ui.mouse_selection.clear();
            ui.help_scroll.scroll_lines(*up, 3);
            return vec![UiAction::RequestDraw(crate::DrawPriority::Interactive)];
        }
    }
    if let TerminalRoute::Pointer(PointerEvent::Wheel { up, column, row }) = &route {
        ui.mouse_selection.clear();
        if ui.pane_resize.is_active() || *column >= size.width || *row >= size.height {
            return Vec::new();
        }
        let mut app = state.lock().unwrap();
        let layout = crate::ui::screen::layout(
            Rect::new(0, 0, size.width, size.height),
            ui.config.message_pane_percent,
            app.preview.fullscreen && app.history_page.is_none(),
        );
        let over_preview = match layout {
            crate::ui::screen::ScreenLayout::Split { preview, .. } => {
                if *column == preview.x {
                    return Vec::new();
                }
                *column > preview.x
            }
            crate::ui::screen::ScreenLayout::PreviewOnly(_) => true,
            crate::ui::screen::ScreenLayout::MainOnly(_) => false,
        };
        if over_preview {
            app.preview.scroll_lines(*up, 3);
            return vec![UiAction::RequestDraw(crate::DrawPriority::Interactive)];
        }
    }
    if state.lock().unwrap().history_page.is_some() {
        if let TerminalRoute::Pointer(pointer) = &route {
            if !matches!(pointer, PointerEvent::Wheel { .. }) {
                let context = {
                    let app = state.lock().unwrap();
                    crate::ui::selection_context(
                        &app,
                        ui.input_page.as_ref(),
                        ui.approval.as_ref(),
                        *ui.help_visible,
                    )
                };
                if !selection_frame.matches_viewport(size.width, size.height)
                    || selection_frame.context() != context
                {
                    ui.mouse_selection.clear();
                    return Vec::new();
                }
                let update = ui.mouse_selection.handle(*pointer, selection_frame);
                return update
                    .copy
                    .map(UiAction::WriteClipboard)
                    .into_iter()
                    .collect();
            }
        }
        ui.mouse_selection.clear();
        let mut app = state.lock().unwrap();
        let Some(page) = app.history_page.as_mut() else {
            return Vec::new();
        };
        let mut changed = false;
        let mut close = false;
        match route {
            TerminalRoute::History(key) => {
                let current = page.offset();
                let half = page.body_height.max(1).div_ceil(2);
                let full = page.body_height.max(1);
                match ui.config.key_mapping.resolve(Scope::History, &key) {
                    Some(Action::Exit) => {
                        close = true;
                        changed = true;
                    }
                    Some(Action::ToggleView) => {
                        page.toggle_view();
                        changed = true;
                    }
                    Some(Action::MoveDown) => {
                        page.set_offset(current.saturating_add(1));
                        changed = true;
                    }
                    Some(Action::MoveUp) => {
                        page.set_offset(current.saturating_sub(1));
                        changed = true;
                    }
                    Some(Action::MoveDownHalf) => {
                        page.set_offset(current.saturating_add(half));
                        changed = true;
                    }
                    Some(Action::MoveUpHalf) => {
                        page.set_offset(current.saturating_sub(half));
                        changed = true;
                    }
                    Some(Action::MoveDownFast) => {
                        page.set_offset(current.saturating_add(full));
                        changed = true;
                    }
                    Some(Action::MoveUpFast) => {
                        page.set_offset(current.saturating_sub(full));
                        changed = true;
                    }
                    _ => {}
                }
            }
            TerminalRoute::Pointer(PointerEvent::Wheel { up, .. }) => {
                let current = page.offset();
                page.set_offset(if up {
                    current.saturating_sub(3)
                } else {
                    current.saturating_add(3)
                });
                changed = true;
            }
            _ => {}
        }
        if close {
            app.history_page = None;
        }
        return changed
            .then_some(UiAction::RequestDraw(crate::DrawPriority::Interactive))
            .into_iter()
            .collect();
    }
    let mut effects = Vec::new();
    match route {
        route @ (TerminalRoute::Pointer(PointerEvent::Wheel { up, .. })
        | TerminalRoute::TranscriptPage { up }) => {
            ui.mouse_selection.clear();
            let page = matches!(route, TerminalRoute::TranscriptPage { .. });
            let before = {
                let mut app = state.lock().unwrap();
                let height = transcript_view_height(
                    size,
                    &app,
                    ui.input,
                    ui.input_page.as_ref(),
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

            let context = {
                let app = state.lock().unwrap();
                crate::ui::selection_context(
                    &app,
                    ui.input_page.as_ref(),
                    ui.approval.as_ref(),
                    *ui.help_visible,
                )
            };
            if !selection_frame.matches_viewport(size.width, size.height)
                || selection_frame.context() != context
            {
                ui.mouse_selection.clear();
                return effects;
            }
            let update = ui.mouse_selection.handle(pointer, selection_frame);
            if let Some(text) = update.copy {
                effects.push(UiAction::WriteClipboard(text));
            }
        }
        TerminalRoute::Paste { text } => {
            ui.mouse_selection.clear();
            paste_text(ui.input, ui.input_page, &text);
        }
        TerminalRoute::ReadClipboard => {
            ui.mouse_selection.clear();
            effects.push(UiAction::ReadClipboard);
        }
        TerminalRoute::Help { action } => {
            ui.mouse_selection.clear();
            let viewport = ui.help_scroll.viewport_rows().max(1);
            let changed = match action {
                Some(Action::Close | Action::Exit) => {
                    *ui.help_visible = false;
                    true
                }
                Some(Action::MoveUp) => {
                    ui.help_scroll.scroll_lines(true, 1);
                    true
                }
                Some(Action::MoveDown) => {
                    ui.help_scroll.scroll_lines(false, 1);
                    true
                }
                Some(Action::MoveUpHalf) => {
                    ui.help_scroll.scroll_lines(true, viewport.div_ceil(2));
                    true
                }
                Some(Action::MoveDownHalf) => {
                    ui.help_scroll.scroll_lines(false, viewport.div_ceil(2));
                    true
                }
                Some(Action::MoveUpFast) => {
                    ui.help_scroll.scroll_lines(true, viewport);
                    true
                }
                Some(Action::MoveDownFast) => {
                    ui.help_scroll.scroll_lines(false, viewport);
                    true
                }
                _ => false,
            };
            if changed {
                effects.push(UiAction::RequestDraw(crate::DrawPriority::Interactive));
            }
        }
        TerminalRoute::OpenHelp => {
            ui.mouse_selection.clear();
            ui.help_scroll.reset();
            *ui.help_visible = true;
        }
        TerminalRoute::Global(action) => {
            ui.mouse_selection.clear();
            match action {
                Action::CopyLink => {
                    let mut app = state.lock().unwrap();
                    app.link_copy.armed =
                        app.session.new_conversation.is_none() && !app.link_copy.links.is_empty();
                }
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
        TerminalRoute::History(_) | TerminalRoute::Ignore => {}
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
            None,
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
            ui.input_page.as_ref(),
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
    if let Some(prompt) = outcome.restore_prompt.take() {
        ui.input.restore_prompt(prompt);
    }
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
        if command.open_help {
            ui.mouse_selection.clear();
            ui.help_scroll.reset();
            *ui.help_visible = true;
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
        if let Some(markdown) = command.copy_markdown {
            outcome.effects.push(UiAction::WriteClipboard(markdown));
        }
        if let Some(history_action) = command.history {
            let request = {
                let mut app = state.lock().unwrap();
                let identity = app
                    .session
                    .session_id
                    .clone()
                    .zip(app.session.session_cwd.clone())
                    .map(
                        |(session_id, cwd)| crate::execution_history::TraceIdentity {
                            frontend: match app.frontend {
                                crate::FrontendKind::Dsh => "e-dsh",
                                crate::FrontendKind::Pi => "e-pi",
                            }
                            .into(),
                            session_id,
                            cwd,
                        },
                    );
                identity.map(|identity| {
                    app.next_history_request_id = app.next_history_request_id.saturating_add(1);
                    let request_id = app.next_history_request_id;
                    let kind = match history_action {
                        crate::command_catalog::FixedSubcommandAction::HistoryShow => {
                            crate::execution_history::HistoryQueryKind::Longest50
                        }
                        crate::command_catalog::FixedSubcommandAction::HistoryPath => {
                            crate::execution_history::HistoryQueryKind::Path
                        }
                        crate::command_catalog::FixedSubcommandAction::HistoryCopy => {
                            crate::execution_history::HistoryQueryKind::Copy
                        }
                        crate::command_catalog::FixedSubcommandAction::HistoryCopy10 => {
                            crate::execution_history::HistoryQueryKind::CopyLongest10
                        }
                    };
                    if kind == crate::execution_history::HistoryQueryKind::Longest50 {
                        app.history_page = Some(crate::history_page::HistoryPage::loading(
                            request_id,
                            identity.session_id.clone(),
                            identity.cwd.clone(),
                        ));
                    }
                    crate::execution_history::HistoryQueryRequest {
                        request_id,
                        identity,
                        kind,
                        input_guard: (kind == crate::execution_history::HistoryQueryKind::Path)
                            .then(|| crate::execution_history::HistoryInputGuard {
                                text: ui.input.buf.clone(),
                                cursor: ui.input.cursor,
                            }),
                        after_offset: 0,
                        watermark: None,
                    }
                })
            };
            if let Some(request) = request {
                ui.mouse_selection.clear();
                outcome.effects.push(UiAction::QueryHistory(request));
            } else {
                state.lock().unwrap().push_error_message(crate::i18n::tr(
                    ui.config.language,
                    "command.history.unavailable",
                ));
            }
        }
        if command.activate_reading {
            let viewport_height = {
                let app = state.lock().unwrap();
                transcript_view_height(
                    size,
                    &app,
                    ui.input,
                    ui.input_page.as_ref(),
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
        if command.config_changed {
            super::effect::sync_live_config(ui.config, state, ui.input, ui.theme);
            outcome
                .effects
                .push(UiAction::PersistConfig(ui.config.clone()));
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
