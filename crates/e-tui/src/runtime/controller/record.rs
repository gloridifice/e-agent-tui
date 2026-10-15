//! Message-scoped temporary state; steering replaces owners without nesting baselines.

use crate::{
    agent::ModelSelection,
    app::{RecordExecution, TemporaryModel, TemporaryModelPhase as Phase},
    interaction::{PendingPrompt, PromptDelivery, PromptState},
    runtime::state::RuntimeState,
    AgentRequest, UiAction,
};

fn execution(app: &RuntimeState, pending: &PendingPrompt) -> RecordExecution {
    RecordExecution {
        id: pending.record_id.expect("record identity"),
        snapshot_pending: false,
        cancel_pending: false,
        snapshot_error: None,
        prompt: pending.prompt.clone(),
        sent: false,
        admitted: false,
        consumed: false,
        skill_consumed: false,
        original: pending.recovery_input(),
        session_id: app.session.session_id.clone(),
        command_pending: false,
        command_name: None,
        native_command_id: None,
        admission_id: None,
        run_observed: false,
        awaiting_run: false,
        recovered: false,
    }
}

fn fail(app: &mut RuntimeState, id: u64, error: String) {
    app.push_error_message(error);
    if let Some(pending) = app.interaction.queue.fail_record(id) {
        if pending.new_mode.is_some() {
            app.restore_new_conversation_input();
        }
        if app.interaction.input.buf.is_empty() {
            app.interaction
                .input
                .restore_prompt(pending.recovery_input());
        } else {
            app.interaction.queue.retain_failed(pending);
        }
    }
}

pub(super) fn captured(
    app: &mut RuntimeState,
    id: u64,
    result: Result<ModelSelection, String>,
    queue: &mut crate::interaction::PendingPromptQueue,
    input: &mut crate::input::InputState,
) {
    if !app.session.macro_execution.as_ref().is_some_and(|owner| {
        owner.id == id
            && (owner.snapshot_pending || owner.cancel_pending)
            && owner.session_id == app.session.session_id
    }) {
        return;
    }
    if app.session.macro_execution.as_ref().unwrap().cancel_pending {
        match result {
            Ok(_) => {
                let owner = app.session.macro_execution.as_mut().unwrap();
                owner.cancel_pending = false;
                owner.awaiting_run = false;
                owner.recovered = true;
            }
            Err(error) => app.push_error_message(format!(
                "Cancellation state is uncertain; cancel again to refresh: {error}"
            )),
        }
        return;
    }
    let error = app
        .session
        .macro_execution
        .as_mut()
        .unwrap()
        .snapshot_error
        .take();
    let queued = queue
        .entries()
        .iter()
        .any(|entry| entry.record_id == Some(id));
    if !queued {
        app.session.macro_execution = None;
        return;
    }
    match error.map_or(result, Err) {
        Ok(original) => {
            app.session.temporary_model = Some(TemporaryModel {
                target: original.clone(),
                original,
                phase: Phase::Ready,
                materializing: false,
            });
            app.session
                .macro_execution
                .as_mut()
                .unwrap()
                .snapshot_pending = false;
        }
        Err(error) => {
            app.push_error_message(error);
            if let Some(pending) = queue.fail_record(id) {
                if pending.new_mode.is_some() {
                    app.restore_new_conversation_input();
                }
                if input.buf.is_empty() {
                    input.restore_prompt(pending.recovery_input());
                } else {
                    queue.retain_failed(pending);
                }
            }
            app.session.macro_execution = None;
        }
    }
}

pub(super) fn observe(app: &mut RuntimeState, fact: &crate::agent::timeline::TimelineFact) {
    use crate::agent::timeline::{ContentBlock, TimelineFact};
    let TimelineFact::UserMessage {
        text,
        source_kind,
        source,
        content,
    } = fact
    else {
        return;
    };
    let session_id = app.session.session_id.clone();
    let Some(owner) = app.session.macro_execution.as_mut().filter(|owner| {
        owner.sent && !owner.consumed && !owner.command_pending && owner.session_id == session_id
    }) else {
        return;
    };
    let matched = if let Some((name, prompt)) = owner.prompt.skill_invocation() {
        if source_kind.as_deref() == Some("skill-invocation")
            && source
                .summary
                .as_deref()
                .is_some_and(|summary| summary.eq_ignore_ascii_case(name))
        {
            owner.skill_consumed = true;
        }
        (owner.skill_consumed
            && (prompt.is_empty() || (source_kind.as_deref() == Some("user") && text == prompt)))
            || (source_kind.as_deref() == Some("user") && owner.prompt.plain_text() == Some(text))
    } else {
        let expected: String = owner
            .prompt
            .parts
            .iter()
            .filter_map(|part| match part {
                crate::PromptPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        source_kind.as_deref() == Some("user")
            && text == &expected
            && content
                .iter()
                .filter(|block| matches!(block, ContentBlock::Image { .. }))
                .count()
                == owner
                    .prompt
                    .parts
                    .iter()
                    .filter(|part| matches!(part, crate::PromptPart::Image(_)))
                    .count()
    };
    if matched {
        owner.consumed = true;
        owner.awaiting_run = !owner.run_observed;
        if owner.admission_id.is_none() {
            owner.admitted = true;
        }
    }
}

pub(super) fn dispatch(app: &mut RuntimeState) -> Option<Vec<UiAction>> {
    let drafting = app.is_new_conversation();
    let asap_only = !drafting && !app.is_fully_idle();
    let mut next = app
        .interaction
        .queue
        .peek_next(asap_only)
        .filter(|pending| !drafting || pending.new_mode.is_some())
        .cloned();
    if app.session.macro_execution.is_none()
        && next.as_ref().is_none_or(|next| next.record_id.is_none())
    {
        return None;
    }
    if app.session.session_id.is_none()
        || (app.session.macro_execution.is_none() && !app.pending_submissions.is_empty())
        || app.interaction.queue.has_backend()
        || app.has_active_command()
        || app
            .session
            .macro_execution
            .as_ref()
            .is_some_and(|owner| owner.snapshot_pending || owner.cancel_pending)
        || super::model::blocked(app)
        || app.is_compacting()
    {
        return Some(Vec::new());
    }
    if let Some(owner) = &app.session.macro_execution {
        if owner.sent {
            let can_replace = owner.admitted
                && owner.consumed
                && !owner.awaiting_run
                && !owner.command_pending
                && !app.has_active_command()
                && !app.interaction.queue.has_backend()
                && next
                    .as_ref()
                    .is_some_and(|next| next.delivery == PromptDelivery::Asap);
            if !can_replace {
                if super::model::should_restore(app, None) {
                    return Some(
                        super::model::restore(app)
                            .map(UiAction::Agent)
                            .into_iter()
                            .collect(),
                    );
                }
                return Some(Vec::new());
            }
        }
    }
    let Some(mut pending) = next.take() else {
        return Some(
            super::model::restore(app)
                .map(UiAction::Agent)
                .into_iter()
                .collect(),
        );
    };
    if pending.new_mode.is_none()
        && !app.is_agent_idle()
        && app.session.status != crate::SessionStatus::Running
    {
        return Some(Vec::new());
    }
    // Legacy marked/plain prompts join an existing Record chain without changing their origin.
    if pending.record_id.is_none() {
        let legacy = app
            .interaction
            .queue
            .take_next(asap_only)
            .expect("queued prompt");
        pending = legacy;
        app.next_prompt_record_id = app
            .next_prompt_record_id
            .checked_add(1)
            .expect("record ID exhausted");
        pending.record_id = Some(app.next_prompt_record_id);
        if let Some(target) = pending.model.take() {
            if pending.macro_input.is_none() {
                pending.macro_input = Some(pending.prompt.clone());
            }
            pending.prompt = super::model::stripped_prompt(&pending.prompt);
            pending.prompt_state = PromptState {
                effort: target.reasoning_effort.clone(),
                model: Some(target),
            };
        }
        app.interaction.queue.push_front(pending.clone());
    }
    let id = pending.record_id.unwrap();
    let same_owner = app
        .session
        .macro_execution
        .as_ref()
        .is_some_and(|owner| owner.id == id);
    if app.session.temporary_model.is_none() {
        let mut owner = execution(app, &pending);
        owner.snapshot_pending = true;
        app.session.macro_execution = Some(owner);
        return Some(vec![UiAction::Agent(AgentRequest::CapturePromptState {
            record_id: id,
        })]);
    }
    let baseline = &app.session.temporary_model.as_ref().unwrap().original;
    let target = match crate::runtime::command::record_route(
        &pending.prompt_state,
        &app.catalogs.model_providers,
        baseline,
    ) {
        Ok(target) => target,
        Err(error) => {
            fail(app, id, error);
            return Some(Vec::new());
        }
    };
    if !same_owner {
        app.session.macro_execution = Some(execution(app, &pending));
    }
    match super::model::select(app, &target) {
        Ok(Some(request)) => return Some(vec![UiAction::Agent(request)]),
        Ok(None) => {}
        Err(key) => {
            fail(app, id, crate::i18n::tr(app.config.language, key));
            return Some(Vec::new());
        }
    }
    let pending = app
        .interaction
        .queue
        .take_next(asap_only)
        .expect("selected Record");
    let was_idle = app.is_agent_idle();
    let steering = pending.delivery == PromptDelivery::Asap
        && (!was_idle || app.interaction.queue.has_backend());
    let running = app.session.status == crate::SessionStatus::Running;
    let owner = app.session.macro_execution.as_mut().unwrap();
    owner.sent = true;
    owner.run_observed = running;
    owner.command_pending = pending.command
        && pending.prompt.skill_invocation().is_none()
        && pending.new_mode.is_none();
    owner.command_name = owner.command_pending.then(|| {
        pending
            .prompt
            .plain_text()
            .unwrap_or_default()
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned()
    });
    owner.awaiting_run = !owner.command_pending;
    let model = app.session.temporary_model.as_mut().unwrap();
    model.phase = Phase::Active;
    model.materializing = pending.new_mode.is_some();
    let request = if let Some(mode) = pending.new_mode {
        AgentRequest::NewInput {
            mode,
            prompt: pending.prompt,
        }
    } else if pending.command {
        crate::runtime::command::backend_command(app, &pending.prompt)
    } else if steering {
        app.interaction.queue.begin_submission(pending.clone());
        AgentRequest::Steer {
            prompt: pending.prompt,
        }
    } else {
        app.admit_submission(&pending.prompt, was_idle);
        AgentRequest::Input {
            prompt: pending.prompt,
        }
    };
    Some(vec![UiAction::Agent(AgentRequest::Record {
        record_id: id,
        request: Box::new(request),
    })])
}
