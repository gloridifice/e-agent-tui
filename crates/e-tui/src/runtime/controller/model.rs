//! Temporary model barriers shared by immediate and queued submissions.

use crate::{
    agent::ModelSelection,
    app::{TemporaryModel, TemporaryModelPhase as Phase},
    interaction::PendingPrompt,
    runtime::state::RuntimeState,
    AgentRequest, PromptInput, PromptPart,
};

pub(super) fn prefixed_prompt(
    app: &RuntimeState,
    prompt: &PromptInput,
) -> Result<Option<ModelSelection>, &'static str> {
    let Some(PromptPart::Text(text)) = prompt.parts.first() else {
        return Ok(None);
    };
    if !text.starts_with("//") {
        return Ok(None);
    }
    let (letter, _) = crate::model_marks::prefix(text).ok_or("model_prefix.invalid")?;
    let (mark, model) = app
        .catalogs
        .marked_model(&app.config.model_marks, letter)
        .ok_or("model_prefix.invalid")?;
    if stripped_prompt(prompt).is_empty() {
        return Err("model_prefix.empty");
    }
    if app.catalogs.current_model.is_none() {
        return Err("model_prefix.loading");
    }
    Ok(Some(ModelSelection {
        provider: mark.provider.clone(),
        model: mark.model.clone(),
        reasoning_effort: crate::catalog::configured_model_effort(
            &app.config.model_default_efforts,
            &mark.provider,
            model,
        ),
    }))
}

pub(super) fn stripped_prompt(prompt: &PromptInput) -> PromptInput {
    let mut prompt = prompt.clone();
    if let Some(PromptPart::Text(text)) = prompt.parts.first_mut() {
        if let Some((_, offset)) = crate::model_marks::prefix(text) {
            text.drain(..offset);
        }
    }
    prompt
        .parts
        .retain(|part| !matches!(part, PromptPart::Text(text) if text.is_empty()));
    prompt
}

fn set_model(selection: &ModelSelection) -> AgentRequest {
    AgentRequest::ModelSet {
        provider: selection.provider.clone(),
        model: selection.model.clone(),
        reasoning_effort: selection.reasoning_effort.clone(),
    }
}

pub(super) fn blocked(app: &RuntimeState) -> bool {
    app.session
        .macro_execution
        .as_ref()
        .is_some_and(|owner| owner.snapshot_pending || owner.cancel_pending)
        || app.session.temporary_model.as_ref().is_some_and(|model| {
            matches!(
                model.phase,
                Phase::Selecting | Phase::Restoring | Phase::RestoreFailed
            )
        })
}

pub(super) fn select(
    app: &mut RuntimeState,
    target: &ModelSelection,
) -> Result<Option<AgentRequest>, &'static str> {
    if app.session.temporary_model.as_ref().is_some_and(|model| {
        model.target == *target
            && matches!(model.phase, Phase::Ready | Phase::Active)
            && (app.session.macro_execution.is_none()
                || app.catalogs.current_model.as_ref().is_some_and(|current| {
                    current.provider == target.provider
                        && current.model == target.model
                        && target
                            .reasoning_effort
                            .as_ref()
                            .is_none_or(|effort| current.reasoning_effort.as_ref() == Some(effort))
                }))
    }) {
        return Ok(None);
    }
    let original = app
        .session
        .temporary_model
        .as_ref()
        .map(|model| model.original.clone())
        .or_else(|| app.catalogs.current_model.clone())
        .ok_or("model_prefix.loading")?;
    app.session.temporary_model = Some(TemporaryModel {
        original,
        target: target.clone(),
        phase: Phase::Selecting,
        materializing: false,
    });
    Ok(Some(set_model(target)))
}

pub(super) fn restore(app: &mut RuntimeState) -> Option<AgentRequest> {
    let model = app.session.temporary_model.as_mut()?;
    if matches!(model.phase, Phase::Restoring | Phase::RestoreFailed) {
        return None;
    }
    model.phase = Phase::Restoring;
    Some(set_model(&model.original))
}

pub(super) fn confirm(app: &mut RuntimeState, current: &ModelSelection) {
    let Some(model) = app.session.temporary_model.as_mut() else {
        return;
    };
    match model.phase {
        Phase::Selecting
            if current.provider == model.target.provider && current.model == model.target.model =>
        {
            if model
                .target
                .reasoning_effort
                .as_ref()
                .is_none_or(|effort| current.reasoning_effort.as_ref() == Some(effort))
            {
                model.phase = Phase::Ready;
            }
        }
        Phase::Restoring | Phase::RestoreFailed if *current == model.original => {
            app.session.temporary_model = None;
            app.session.macro_execution = None;
        }
        _ => {}
    }
}

pub(super) fn should_restore(app: &RuntimeState, next: Option<&PendingPrompt>) -> bool {
    let Some(model) = &app.session.temporary_model else {
        return false;
    };
    let creating = model.materializing
        && app
            .session
            .new_conversation
            .as_ref()
            .is_some_and(|draft| draft.pending_input.is_some());
    if blocked(app)
        || creating
        || !app.is_agent_idle()
        || app.interaction.queue.has_backend()
        || (app.session.macro_execution.is_some() && app.has_active_command())
        || app
            .session
            .macro_execution
            .as_ref()
            .is_some_and(|execution| {
                execution.command_pending
                    || execution.awaiting_run
                    || (execution.sent && !execution.admitted && !execution.recovered)
            })
    {
        return false;
    }
    model.phase == Phase::Active
        || next.and_then(|pending| pending.model.as_ref()) != Some(&model.target)
}
