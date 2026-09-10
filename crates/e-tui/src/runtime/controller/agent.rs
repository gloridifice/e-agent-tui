//! Session, catalog, interaction, and normalized agent-error controller behavior.

use super::{
    normalized_session_status, AgentEvent, ApprovalCard, Arc, DrawPriority, InputPageSession,
    Instant, LoginView, Mutex, NewMode, QuestionBatch, RuntimeState, RuntimeUiState, ScrollState,
    UiAction,
};
use crate::i18n::tr_args;

pub(super) fn apply_agent(
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
            app.refresh_link_copy();
            app.take_actions()
        }
        AgentEvent::Timeline(TimelineEvent::Append(record)) => {
            let mut app = state.lock().unwrap();
            let commits_new_conversation = matches!(
                &record.fact,
                crate::agent::timeline::TimelineFact::UserMessage {
                    source_kind: Some(kind),
                    ..
                } if kind == "user" || kind == "skill-invocation"
            );
            if commits_new_conversation
                && app
                    .session
                    .new_conversation
                    .as_ref()
                    .is_some_and(|draft| draft.attached && draft.pending_input.is_some())
            {
                app.session.new_conversation = None;
            }
            app.apply_host_event(&record);
            use crate::agent::timeline::TimelineFact;
            if matches!(
                record.fact,
                TimelineFact::AssistantMessage { .. }
                    | TimelineFact::UserMessage { .. }
                    | TimelineFact::TurnEnd { .. }
            ) || matches!(
                record.surface,
                Some(crate::agent::timeline::SurfaceOperation::Replace { .. })
            ) || (app.link_copy.owner.is_some()
                && matches!(record.fact, TimelineFact::AssistantChunk { .. }))
            {
                app.refresh_link_copy();
            }
            app.take_actions()
        }
        AgentEvent::Timeline(TimelineEvent::History { records, has_more }) => {
            let mut app = state.lock().unwrap();
            app.prepend_host_events(&records);
            app.session.history_loading = false;
            app.session.history_exhausted = !has_more;
            app.take_actions()
        }
        AgentEvent::Session(event) => apply_session(event, state, ui),
        AgentEvent::Catalog(event) => apply_catalog(event, state, ui),
        AgentEvent::Interaction(InteractionEvent::SetEditorText { text }) => {
            if ui.input_page.is_none() {
                ui.input.restore_text(text);
                vec![UiAction::RequestDraw(DrawPriority::Interactive)]
            } else {
                Vec::new()
            }
        }
        AgentEvent::Interaction(InteractionEvent::AsapQueue {
            session_id,
            prompts,
            operation,
            error,
        }) => {
            if state.lock().unwrap().session.session_id.as_deref() != Some(&session_id) {
                return Vec::new();
            }
            ui.queue.update_remote(prompts);
            match operation {
                Some(crate::agent::AsapQueueOperation::Submit) => {
                    ui.queue.complete_submission(error.is_some())
                }
                Some(crate::agent::AsapQueueOperation::Clear) => ui.queue.complete_clear(),
                None => {}
            }
            if let Some(error) = error {
                state.lock().unwrap().push_error_message(error);
            }
            ui.queue
                .take_clear_request()
                .then_some(UiAction::Agent(crate::AgentRequest::ClearAsap))
                .into_iter()
                .collect()
        }
        AgentEvent::Interaction(event) => apply_interaction(event, state, ui),
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
                .then_some(UiAction::RequestDraw(DrawPriority::Content))
                .into_iter()
                .collect()
        }
        AgentEvent::EffectCompleted(result) => {
            let dirty =
                super::RuntimeController::apply_effect_result(result, state, Instant::now());
            dirty
                .then_some(UiAction::RequestDraw(DrawPriority::Content))
                .into_iter()
                .collect()
        }
        AgentEvent::Deadline(_) => Vec::new(),
    }
}

pub(super) fn apply_session(
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
                    app.history_page = None;
                    app.interaction.question = None;
                }
                app.render.status_flashes = Default::default();
                app.session.session_id = Some(attached.id.clone());
                let materialization_pending = app
                    .session
                    .new_conversation
                    .as_ref()
                    .is_some_and(|draft| draft.pending_input.is_some());
                if !materialization_pending {
                    app.session.temporary_model = None;
                    app.session.new_conversation = None;
                } else if switched {
                    if let Some(draft) = app.session.new_conversation.as_mut() {
                        draft.attached = true;
                    }
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
            let mut app = state.lock().unwrap();
            app.refresh_link_copy();
            let mut actions = app.take_actions();
            actions.push(UiAction::PersistSessionId(attached.id));
            actions
        }
        SessionEvent::Status(status) => {
            let mut app = state.lock().unwrap();
            app.session.status = normalized_session_status(&status);
            if app.session.status == crate::SessionStatus::Idle
                && app.pending_submissions.is_empty()
            {
                app.stop_thinking();
            }
            Vec::new()
        }
        SessionEvent::Title(title) => {
            state.lock().unwrap().session.session_title = Some(title);
            Vec::new()
        }
        SessionEvent::Cost { session_id, usd } => {
            let mut app = state.lock().unwrap();
            if app.session.session_id.as_deref() == Some(session_id.as_str()) {
                app.session.cost_usd = usd.filter(|cost| cost.is_finite() && *cost >= 0.0);
            }
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

pub(super) fn apply_catalog(
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
            app.render.status_flashes.observe(&catalogs, Instant::now());
            if let Some(current) = current {
                super::model::confirm(&mut app, &current);
                app.session.provider = Some(current.provider);
                app.session.model = Some(current.model);
            }
            drop(app);
            ui.input.catalog_changed(&catalogs);
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_model(providers, selected);
                page.apply_effort(&catalogs);
            }
        }
    }
    Vec::new()
}

pub(super) fn apply_interaction(
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
            state.lock().unwrap().history_page = None;
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
            state.lock().unwrap().history_page = None;
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
            return apply_agent_error(&code, &message, state, ui);
        }
        InteractionEvent::Heartbeat
        | InteractionEvent::SetEditorText { .. }
        | InteractionEvent::AsapQueue { .. } => {}
    }
    Vec::new()
}

pub(super) fn apply_agent_error(
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
        "model-failed" | "pi-rpc-set_model" | "pi-rpc-set_thinking_level" | "pi-rpc-get_state"
    ) {
        let mut app = state.lock().unwrap();
        if let Some(model) = app.session.temporary_model.clone() {
            use crate::app::TemporaryModelPhase as Phase;
            if matches!(model.phase, Phase::Selecting | Phase::Restoring) {
                let language = app.config.language;
                app.push_error_message(tr_args(
                    language,
                    "controller.runtime_error",
                    &[("code", code.to_owned()), ("message", message.to_owned())],
                ));
                if model.phase == Phase::Restoring {
                    app.session.temporary_model.as_mut().unwrap().phase = Phase::RestoreFailed;
                    app.push_error_message(tr_args(
                        language,
                        "model_prefix.restore_failed",
                        &[(
                            "model",
                            format!("{}/{}", model.original.provider, model.original.model),
                        )],
                    ));
                    return Vec::new();
                }
                if let Some(pending) = ui.queue.fail_model(&model.target) {
                    if pending.new_mode.is_some() {
                        app.restore_new_conversation_input();
                    }
                    if ui.input.buf.is_empty() {
                        ui.input.restore_prompt(pending.prompt);
                    } else {
                        ui.queue.retain_failed(pending);
                    }
                }
                return super::model::restore(&mut app)
                    .map(UiAction::Agent)
                    .into_iter()
                    .collect();
            }
        }
    }
    if matches!(
        code,
        "new-failed"
            | "new-input-failed"
            | "image-input-too-large"
            | "skill-unknown"
            | "skill-unavailable"
            | "skill-failed"
            | "pi-rpc-prompt"
    ) && state.lock().unwrap().is_new_conversation()
    {
        let restored = {
            let mut app = state.lock().unwrap();
            let restored = app.restore_new_conversation_input();
            if restored.is_some() {
                let language = app.config.language;
                app.set_new_conversation_notice(tr_args(
                    language,
                    "controller.new_failed",
                    &[("message", message.to_owned())],
                ));
            }
            restored
        };
        if let Some(prompt) = restored {
            ui.input.restore_prompt(prompt);
            ui.input.multiline = ui.input.buf.contains('\n');
        } else {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.push_error_message(tr_args(
                language,
                "controller.runtime_error",
                &[("code", code.to_owned()), ("message", message.to_owned())],
            ));
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
        let pending = std::mem::take(&mut app.pending_submissions);
        let pending_count = pending.len();
        app.pending_submissions = pending.into_iter().filter(|id| {
            !app.transcript.get(id).is_some_and(|node| matches!(&node.item,
                crate::display::DisplayItem::Card(card) if card.role == crate::display::CardRole::Skill))
        }).collect();
        if pending_count > 0
            && app.pending_submissions.is_empty()
            && app.session.status == crate::SessionStatus::Idle
        {
            app.stop_thinking();
        }
    }
    if matches!(
        code,
        "input-failed"
            | "image-input-failed"
            | "image-input-too-large"
            | "skill-unknown"
            | "skill-unavailable"
            | "skill-failed"
            | "pi-rpc-prompt"
    ) {
        app.pending_submissions.clear();
        if app.session.status == crate::SessionStatus::Idle {
            app.stop_thinking();
        }
    }
    let language = app.config.language;
    app.push_error_message(tr_args(
        language,
        "controller.runtime_error",
        &[("code", code.to_owned()), ("message", message.to_owned())],
    ));
    Vec::new()
}
