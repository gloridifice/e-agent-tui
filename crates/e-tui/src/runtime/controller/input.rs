//! Composer, Input Page, approval, and queued-prompt controller behavior.

use super::{
    agent_action, AgentRequest, ApprovalCard, ControllerAction, InputAction, InputHandlerOutcome,
    InputPageSession, InputPageUiState, KeyEvent, Mutex, PageEffect, PendingCommand, PromptInput,
    RuntimeState, UiAction,
};
use crate::{
    i18n::tr,
    interaction::{PendingPromptQueue, PromptDelivery},
    theme,
};

pub(super) fn apply_action(action: ControllerAction, input_page: &mut Option<InputPageSession>) {
    match action {
        ControllerAction::OpenPage(page) => *input_page = Some(page),
        ControllerAction::ClosePage => *input_page = None,
    }
}

fn submit_prompt(
    prompt: PromptInput,
    delivery: PromptDelivery,
    state: &Mutex<RuntimeState>,
    queue: &mut PendingPromptQueue,
    outcome: &mut InputHandlerOutcome,
) {
    let target = {
        let app = state.lock().unwrap();
        super::model::prefixed_prompt(&app, &prompt)
    };
    let target = match target {
        Ok(target) => target,
        Err(key) => {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.push_error_message(tr(language, key));
            outcome.restore_prompt = Some(prompt);
            return;
        }
    };
    if state.lock().unwrap().session.macro_execution.is_some() {
        let prompt_state = crate::interaction::PromptState {
            effort: target
                .as_ref()
                .and_then(|target| target.reasoning_effort.clone()),
            model: target.clone(),
        };
        let expanded = if target.is_some() {
            super::model::stripped_prompt(&prompt)
        } else {
            prompt.clone()
        };
        enqueue_record(
            expanded,
            prompt,
            prompt_state,
            delivery,
            state,
            queue,
            outcome,
        );
        return;
    }
    let defer_new = {
        let mut app = state.lock().unwrap();
        app.fold_shell_output();
        app.is_new_conversation() && app.session.temporary_model.is_some()
    };
    if target.is_some() || defer_new {
        let mut app = state.lock().unwrap();
        let mut pending = crate::interaction::PendingPrompt::new(prompt.clone(), delivery);
        pending.model = target;
        if app.is_new_conversation() {
            let Some(AgentRequest::NewInput { mode, .. }) =
                app.materialize_new_conversation(super::model::stripped_prompt(&prompt))
            else {
                outcome.restore_prompt = Some(prompt);
                return;
            };
            pending.new_mode = Some(mode);
        }
        queue.push_pending(pending);
        return;
    }
    let new_input = {
        let mut state = state.lock().unwrap();
        if state.is_new_conversation() && state.session.temporary_model.is_none() {
            state.materialize_new_conversation(prompt.clone())
        } else {
            None
        }
    };
    if let Some(message) = new_input {
        outcome.effects.push(agent_action(message));
        return;
    }
    let is_draft = state.lock().unwrap().is_new_conversation();
    if is_draft {
        let mut app = state.lock().unwrap();
        let language = app.config.language;
        app.set_new_conversation_notice(tr(language, "command.new.in_progress"));
        return;
    }
    let immediate = {
        let mut state = state.lock().unwrap();
        let immediate = state.enqueue_or_immediate(prompt.clone(), delivery, queue);
        if immediate {
            state.admit_submission(&prompt, true);
        }
        immediate
    };
    if immediate {
        outcome
            .effects
            .push(agent_action(AgentRequest::Input { prompt }));
    }
}

pub(super) fn enqueue_record(
    prompt: PromptInput,
    original: PromptInput,
    prompt_state: crate::interaction::PromptState,
    delivery: PromptDelivery,
    state: &Mutex<RuntimeState>,
    queue: &mut PendingPromptQueue,
    outcome: &mut InputHandlerOutcome,
) {
    let mut app = state.lock().unwrap();
    let mut pending = crate::interaction::PendingPrompt::new(prompt.clone(), delivery);
    pending.command = prompt
        .plain_text()
        .is_some_and(|text| text.starts_with('/'));
    pending.macro_input = Some(original.clone());
    pending.prompt_state = prompt_state;
    app.next_prompt_record_id = app
        .next_prompt_record_id
        .checked_add(1)
        .expect("record ID exhausted");
    pending.record_id = Some(app.next_prompt_record_id);
    if app.is_new_conversation() {
        if pending.command && prompt.skill_invocation().is_none() {
            app.push_error_message(
                "Send a prompt or explicit skill first to create the new session",
            );
            outcome.restore_prompt = Some(original);
            return;
        }
        let Some(AgentRequest::NewInput { mode, .. }) = app.materialize_new_conversation(prompt)
        else {
            outcome.restore_prompt = Some(original);
            return;
        };
        pending.new_mode = Some(mode);
    }
    app.fold_shell_output();
    queue.push_pending(pending);
}

pub(super) fn apply_input_action(
    action: InputAction,
    state: &Mutex<RuntimeState>,
    queue: &mut PendingPromptQueue,
) -> InputHandlerOutcome {
    let mut outcome = InputHandlerOutcome::default();
    match action {
        InputAction::None | InputAction::ToggleMultiline => {}
        InputAction::Macro {
            entry,
            original,
            after_turn,
        } => {
            let compatible = crate::runtime::command::temporary_route_compatible(&entry.prompt);
            let overrides = entry.model.is_some() || entry.effort.is_some();
            if overrides && !compatible {
                state.lock().unwrap().push_error_message(
                    "This macro body cannot be combined with temporary model/effort options",
                );
                outcome.restore_prompt = Some(original);
                return outcome;
            }
            let prompt_state = {
                let app = state.lock().unwrap();
                crate::runtime::command::macro_state(
                    &entry,
                    &app.config,
                    &app.catalogs.model_providers,
                )
            };
            match prompt_state {
                Err(error) => {
                    state.lock().unwrap().push_error_message(error);
                    outcome.restore_prompt = Some(original);
                }
                Ok(_) if !compatible => {
                    let action =
                        crate::input::InputState::classify(PromptInput::text(entry.prompt));
                    let action = match action {
                        InputAction::Send(prompt) if after_turn => {
                            InputAction::SendAfterTurn(prompt)
                        }
                        action => action,
                    };
                    let queued = queue.len();
                    let admissions = state.lock().unwrap().pending_submissions.len();
                    outcome = apply_input_action(action, state, queue);
                    if queue.len() > queued {
                        queue.capture_macro_input(original.clone());
                    } else if outcome.effects.iter().any(|effect| {
                        matches!(
                            effect,
                            UiAction::Agent(
                                AgentRequest::Input { .. } | AgentRequest::NewInput { .. }
                            )
                        )
                    }) {
                        let mut app = state.lock().unwrap();
                        if app.is_new_conversation() {
                            app.session.macro_draft_input = Some(original.clone());
                        } else if let Some(id) = app.pending_submissions.get(admissions).cloned() {
                            app.session.macro_admissions.push((id, original.clone()));
                        }
                    }
                    if outcome.restore_prompt.is_some() {
                        outcome.restore_prompt = Some(original.clone())
                    }
                    if let Some(command) = &mut outcome.command {
                        command.original = original
                    }
                }
                Ok(prompt_state) => {
                    let delivery = if after_turn || state.lock().unwrap().is_compacting() {
                        PromptDelivery::AfterTurn
                    } else {
                        PromptDelivery::Asap
                    };
                    enqueue_record(
                        PromptInput::text(entry.prompt),
                        original,
                        prompt_state,
                        delivery,
                        state,
                        queue,
                        &mut outcome,
                    );
                }
            }
        }
        InputAction::Send(prompt) => {
            let delivery = {
                let app = state.lock().unwrap();
                if !app.is_new_conversation() && app.is_compacting() {
                    PromptDelivery::AfterTurn
                } else {
                    PromptDelivery::Asap
                }
            };
            submit_prompt(prompt, delivery, state, queue, &mut outcome)
        }
        InputAction::SendAfterTurn(prompt) => submit_prompt(
            prompt,
            PromptDelivery::AfterTurn,
            state,
            queue,
            &mut outcome,
        ),
        InputAction::Command {
            line,
            images,
            original,
        } => {
            let prompt = PromptInput::text(line.clone());
            if images.is_empty()
                && prompt.skill_invocation().is_some()
                && state.lock().unwrap().session.macro_execution.is_some()
            {
                let delivery = if state.lock().unwrap().is_compacting() {
                    PromptDelivery::AfterTurn
                } else {
                    PromptDelivery::Asap
                };
                enqueue_record(
                    prompt,
                    original,
                    Default::default(),
                    delivery,
                    state,
                    queue,
                    &mut outcome,
                );
            } else {
                outcome.command = Some(PendingCommand {
                    line,
                    images,
                    original,
                });
            }
        }
        InputAction::Shell { command, original } => {
            let mut app = state.lock().unwrap();
            let error = if original.has_images() {
                Some("terminal.command_no_images")
            } else if command.trim().is_empty() {
                Some("shell.empty")
            } else if app.shell.active.is_some() {
                Some("shell.busy")
            } else {
                None
            };
            if let Some(error) = error {
                let message = tr(app.config.language, error);
                let drafting = app.is_new_conversation();
                app.push_error_message(message);
                if drafting {
                    if let Some(id) = app.transcript.nodes().last().map(|node| node.id().clone()) {
                        app.shell.draft_ids.insert(id);
                    }
                }
                outcome.restore_prompt = Some(original);
            } else {
                outcome
                    .effects
                    .push(UiAction::StartShell(app.start_shell(command)));
            }
        }
        InputAction::Interrupt => {
            if let Some(id) = state.lock().unwrap().shell.active.clone() {
                outcome.effects.push(UiAction::CancelShell(id));
                return outcome;
            }
            queue.clear();
            let record_id = {
                let mut app = state.lock().unwrap();
                app.pending_submissions.clear();
                app.session
                    .macro_execution
                    .as_mut()
                    .filter(|owner| owner.sent)
                    .map(|owner| {
                        owner.cancel_pending = true;
                        owner.id
                    })
            };
            outcome.effects.push(agent_action(AgentRequest::Interrupt));
            if let Some(record_id) = record_id {
                outcome
                    .effects
                    .push(agent_action(AgentRequest::CapturePromptState { record_id }));
            }
        }
        InputAction::Quit => {
            if state.lock().unwrap().session.macro_execution.is_some() {
                state
                    .lock()
                    .unwrap()
                    .push_error_message("Wait for the prompt macro route to be restored");
            } else {
                outcome.effects.push(UiAction::Quit)
            }
        }
        InputAction::PreviewToggle => {
            let mut app = state.lock().unwrap();
            app.preview.fullscreen = !app.preview.fullscreen;
        }
        InputAction::ReadingToggle => outcome.activate_reading = true,
    }
    outcome
}

pub(super) fn answer_approval(
    action: Option<crate::key_mapping::Action>,
    approval: &mut Option<ApprovalCard>,
) -> Vec<UiAction> {
    let allow = match action {
        Some(crate::key_mapping::Action::Allow) => true,
        Some(crate::key_mapping::Action::Deny) => false,
        _ => return Vec::new(),
    };
    let Some(card) = approval.take() else {
        return Vec::new();
    };
    vec![UiAction::Agent(card.answer(allow))]
}

/// Apply one Input Page key synchronously and return only lock-external work.
/// Config persistence owns a cloned snapshot so the runner need not borrow
/// controller state.
pub(super) fn apply_input_page_key(
    key: &KeyEvent,
    state: &Mutex<RuntimeState>,
    ui: &mut InputPageUiState<'_>,
) -> Vec<UiAction> {
    let page = ui
        .input_page
        .as_ref()
        .expect("input-page handler requires an open page");
    if ui.config.key_mapping.resolve(page.key_scope(), key)
        == Some(crate::key_mapping::Action::Paste)
    {
        return vec![UiAction::ReadClipboard];
    }
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
            PageEffect::Send(message) => {
                let exclusive = state.lock().unwrap().session.macro_execution.is_some();
                if exclusive
                    && matches!(
                        message,
                        AgentRequest::ModelSet { .. }
                            | AgentRequest::Attach { .. }
                            | AgentRequest::Command { .. }
                            | AgentRequest::AuthStart { .. }
                            | AgentRequest::Mcp(_)
                    )
                {
                    state
                        .lock()
                        .unwrap()
                        .push_error_message("Wait for the prompt macro route to be restored");
                } else {
                    effects.push(UiAction::Agent(message));
                }
            }
            PageEffect::WriteClipboard(value) => effects.push(UiAction::WriteClipboard(value)),
            PageEffect::ConfigChanged => {
                ui.config.resolved_theme = theme::resolve(&ui.config.theme, ui.themes);
                super::effect::sync_live_config(ui.config, state, ui.input, ui.theme);
                effects.push(UiAction::PersistConfig(ui.config.clone()));
            }
        }
    }
    if outcome.close {
        apply_action(ControllerAction::ClosePage, ui.input_page);
    }
    effects
}
