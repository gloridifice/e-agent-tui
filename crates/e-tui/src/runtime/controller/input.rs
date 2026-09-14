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
    let defer_new = {
        let app = state.lock().unwrap();
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

pub(super) fn apply_input_action(
    action: InputAction,
    state: &Mutex<RuntimeState>,
    queue: &mut PendingPromptQueue,
) -> InputHandlerOutcome {
    let mut outcome = InputHandlerOutcome::default();
    match action {
        InputAction::None | InputAction::ToggleMultiline => {}
        InputAction::Send(prompt) => {
            submit_prompt(prompt, PromptDelivery::Asap, state, queue, &mut outcome)
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
            outcome.command = Some(PendingCommand {
                line,
                images,
                original,
            })
        }
        InputAction::Interrupt => {
            queue.clear();
            state.lock().unwrap().pending_submissions.clear();
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
            PageEffect::Send(message) => effects.push(UiAction::Agent(message)),
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
