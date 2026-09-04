//! Composer, Input Page, approval, and queued-prompt controller behavior.

use super::{
    agent_action, AgentRequest, ApprovalCard, ControllerAction, InputAction, InputHandlerOutcome,
    InputPageSession, InputPageUiState, KeyCode, KeyEvent, Mutex, PageEffect, PendingCommand,
    PromptInput, RuntimeState, UiAction,
};
use crate::{i18n::tr, theme};

pub(super) fn apply_action(action: ControllerAction, input_page: &mut Option<InputPageSession>) {
    match action {
        ControllerAction::OpenPage(page) => *input_page = Some(page),
        ControllerAction::ClosePage => *input_page = None,
    }
}

pub(super) fn apply_input_action(
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
                let mut app = state.lock().unwrap();
                let language = app.config.language;
                app.set_new_conversation_notice(tr(language, "command.new.in_progress"));
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

pub(super) fn answer_approval(
    key: &KeyEvent,
    approval: &mut Option<ApprovalCard>,
) -> Vec<UiAction> {
    let allow = matches!(key.code, KeyCode::Char('y' | 'Y'));
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
