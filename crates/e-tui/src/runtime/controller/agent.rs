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
        AgentEvent::Mcp(event) => {
            let mut app = state.lock().unwrap();
            match event {
                crate::mcp::McpEvent::Available(available) => app.mcp_available = available,
                crate::mcp::McpEvent::Saved { message } => app.push_system_message(message),
                crate::mcp::McpEvent::ReloadRequired(required) => {
                    app.mcp_reload_required = required
                }
                event => {
                    if let Some(menu) = &mut app.mcp {
                        menu.apply(event);
                    }
                }
            }
            vec![UiAction::RequestDraw(DrawPriority::Interactive)]
        }
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
            use crate::agent::timeline::TimelineFact;
            if commits_new_conversation {
                app.session.macro_draft_input = None;
            }
            super::record::observe(&mut app, &record.fact);
            let session_id = app.session.session_id.clone();
            if let TimelineFact::CommandStarted { id, name, .. } = &record.fact {
                if let Some(execution) = app.session.macro_execution.as_mut() {
                    if execution.command_pending
                        && execution.session_id == session_id
                        && execution
                            .command_name
                            .as_deref()
                            .is_some_and(|expected| expected.eq_ignore_ascii_case(name))
                        && execution.native_command_id.is_none()
                    {
                        execution.native_command_id = Some(id.clone());
                    }
                }
            }
            app.apply_host_event(&record);
            let pending = app.pending_submissions.clone();
            app.session
                .macro_admissions
                .retain(|(id, _)| pending.contains(id));
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
                    let sending = ui.queue.sending_record_id();
                    if let Some(owner) = state
                        .lock()
                        .unwrap()
                        .session
                        .macro_execution
                        .as_mut()
                        .filter(|owner| Some(owner.id) == sending)
                    {
                        owner.admitted = true;
                        if error.is_some() {
                            owner.awaiting_run = false;
                            owner.recovered = true;
                        }
                    }
                    ui.queue.complete_submission(error.is_some())
                }
                Some(crate::agent::AsapQueueOperation::Clear) => {
                    ui.queue.complete_clear();
                    if error.is_none() {
                        if let Some(owner) = state
                            .lock()
                            .unwrap()
                            .session
                            .macro_execution
                            .as_mut()
                            .filter(|owner| owner.sent && owner.admitted && !owner.consumed)
                        {
                            owner.awaiting_run = false;
                            owner.consumed = true;
                            owner.recovered = true;
                        }
                    }
                }
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
                    app.session.macro_admissions.clear();
                    let initial = app.session.session_id.is_none();
                    let shells: Vec<_> = app
                        .transcript
                        .nodes()
                        .iter()
                        .filter(|node| {
                            initial
                                && matches!(&node.item,
                            crate::display::DisplayItem::Composite { detail, .. }
                                if detail.role == crate::display::CardRole::Terminal)
                        })
                        .map(|node| node.item.clone())
                        .collect();
                    let active = if initial {
                        app.shell.active.take()
                    } else {
                        None
                    };
                    let unfolded = if initial {
                        std::mem::take(&mut app.shell.unfolded)
                    } else {
                        Default::default()
                    };
                    app.reset_transcript();
                    app.shell.active = active;
                    app.shell.unfolded = unfolded;
                    for item in shells {
                        if let crate::display::DisplayItem::Composite { detail, .. } = &item {
                            if let Some(unit) = detail.unit {
                                app.render.units.insert(unit, detail.copy_source.clone());
                                app.render.next_unit = app.render.next_unit.max(unit + 1);
                            }
                        }
                        app.transcript.append(item, None);
                    }
                    app.history_page = None;
                    app.mcp = None;
                    app.mcp_available = false;
                    app.mcp_reload_required = false;
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
                    app.session.macro_execution = None;
                    app.session.macro_draft_input = None;
                    app.session.new_conversation = None;
                } else if switched {
                    if let Some(draft) = app.session.new_conversation.as_mut() {
                        draft.attached = true;
                    }
                }
                if let Some(execution) = app.session.macro_execution.as_mut() {
                    execution.session_id = Some(attached.id.clone());
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
            let running = app.session.status == crate::SessionStatus::Running;
            let macro_active = app
                .session
                .temporary_model
                .as_ref()
                .is_some_and(|model| model.phase == crate::app::TemporaryModelPhase::Active);
            if macro_active {
                if let Some(execution) = app.session.macro_execution.as_mut() {
                    if running {
                        execution.run_observed = true;
                        if execution.consumed {
                            execution.awaiting_run = false;
                        }
                    } else if !execution.consumed {
                        execution.run_observed = false;
                    }
                }
            }
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
            refresh_input_catalogs(state, ui, &catalogs);
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
            refresh_input_catalogs(state, ui, &catalogs);
        }
        CatalogEvent::Commands(commands) => {
            let catalogs = {
                let mut app = state.lock().unwrap();
                app.catalogs.integrated_commands = commands;
                app.catalogs.clone()
            };
            refresh_input_catalogs(state, ui, &catalogs);
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
        CatalogEvent::Authentication {
            providers,
            provider_ref,
            logout,
            error,
        } => {
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_auth_catalog(providers, provider_ref, logout, error);
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
            let effort_mismatch = current.as_ref().is_some_and(|current| {
                app.session.temporary_model.as_ref().is_some_and(|model| {
                    model.phase == crate::app::TemporaryModelPhase::Selecting
                        && current.provider == model.target.provider
                        && current.model == model.target.model
                        && model
                            .target
                            .reasoning_effort
                            .as_ref()
                            .is_some_and(|effort| current.reasoning_effort.as_ref() != Some(effort))
                })
            });
            if let Some(current) = current {
                super::model::confirm(&mut app, &current);
                app.session.provider = Some(current.provider);
                app.session.model = Some(current.model);
            }
            drop(app);
            if effort_mismatch {
                return apply_agent_error(
                    "model-failed",
                    "The requested macro/temporary effort was not confirmed",
                    state,
                    ui,
                );
            }
            refresh_input_catalogs(state, ui, &catalogs);
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_model(providers, selected);
                page.apply_effort(&catalogs);
            }
        }
    }
    Vec::new()
}

fn refresh_input_catalogs(
    state: &Mutex<RuntimeState>,
    ui: &mut RuntimeUiState<'_>,
    catalogs: &crate::CatalogModel,
) {
    let app = state.lock().unwrap();
    ui.input
        .catalog_changed_with_macros(catalogs, &app.config.prompt_macros);
}

pub(super) fn apply_interaction(
    event: crate::agent::InteractionEvent,
    state: &Arc<Mutex<RuntimeState>>,
    ui: &mut RuntimeUiState<'_>,
) -> Vec<UiAction> {
    use crate::agent::InteractionEvent;

    match event {
        InteractionEvent::PromptStateCaptured {
            record_id,
            session_id,
            result,
        } => {
            let mut app = state.lock().unwrap();
            if app.session.session_id.as_deref() == Some(&session_id) {
                super::record::captured(&mut app, record_id, result, ui.queue, ui.input);
            }
        }
        InteractionEvent::Heartbeat => {}
        InteractionEvent::PromptDispatched {
            record_id,
            request_id,
            session_id,
        } => {
            let mut app = state.lock().unwrap();
            if app.session.session_id.as_deref() == Some(&session_id) {
                if let Some(owner) = app.session.macro_execution.as_mut().filter(|owner| {
                    Some(owner.id) == record_id && owner.sent && owner.admission_id.is_none()
                }) {
                    owner.admission_id = Some(request_id);
                    owner.admitted = false;
                }
            }
        }
        InteractionEvent::PromptAdmission {
            request_id,
            session_id,
            expects_run,
        } => {
            let mut app = state.lock().unwrap();
            if app.session.session_id.as_deref() != Some(&session_id) {
                return Vec::new();
            }
            let Some(execution) = app.session.macro_execution.as_mut() else {
                return Vec::new();
            };
            if execution.session_id.as_deref() != Some(&session_id)
                || execution.admission_id.as_deref() != Some(request_id.as_str())
            {
                return Vec::new();
            }
            execution.admitted = true;
            execution.awaiting_run =
                expects_run && (!execution.consumed || !execution.run_observed);
            if !expects_run {
                execution.consumed = true;
                execution.command_pending = true;
                execution.native_command_id = Some(request_id);
                app.pending_submissions.clear();
                if app
                    .session
                    .new_conversation
                    .as_ref()
                    .is_some_and(|draft| draft.attached)
                {
                    app.session.new_conversation = None;
                }
                if app.session.status == crate::SessionStatus::Idle {
                    app.stop_thinking();
                }
            }
        }
        InteractionEvent::PromptSettlement {
            request_id,
            session_id,
            confirmed,
        } => {
            let mut app = state.lock().unwrap();
            if app.session.session_id.as_deref() != Some(&session_id) {
                return Vec::new();
            }
            if let Some(execution) = app.session.macro_execution.as_mut() {
                if execution.session_id.as_deref() == Some(&session_id)
                    && execution.admission_id.as_deref() == Some(&request_id)
                    && execution.command_pending
                {
                    if confirmed {
                        execution.command_pending = false;
                    } else {
                        app.push_error_message("Prompt settlement could not be confirmed; its macro route remains held. Cancel to refresh native state.");
                    }
                }
            }
        }
        InteractionEvent::CommandResult { id, outcome, text } => {
            let mut app = state.lock().unwrap();
            if outcome == "unconfirmed" {
                app.push_error_message("Command settlement could not be confirmed; queued work remains held. Cancel to refresh native state.");
                return Vec::new();
            }
            let session_id = app.session.session_id.clone();
            let mut recovery = None;
            if let Some(execution) = app.session.macro_execution.as_mut() {
                if execution.command_pending && execution.session_id == session_id {
                    if !execution
                        .native_command_id
                        .iter()
                        .chain(execution.admission_id.iter())
                        .any(|expected| expected == &id)
                    {
                        return Vec::new();
                    }
                    execution.command_pending = false;
                    execution.admitted = true;
                    execution.consumed = true;
                    if outcome == "error" && !execution.recovered {
                        execution.awaiting_run = false;
                        execution.recovered = true;
                        recovery = Some(execution.original.clone());
                    }
                }
            }
            app.finish_command_execution();
            app.apply_command_result(&id, &outcome, text.as_deref());
            drop(app);
            if let Some(original) = recovery {
                restore_macro_prompt(ui, original);
            }
        }
        InteractionEvent::Approval {
            id, label, reason, ..
        } => {
            {
                let mut app = state.lock().unwrap();
                app.history_page = None;
                app.mcp = None;
            }
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
            {
                let mut app = state.lock().unwrap();
                app.history_page = None;
                app.mcp = None;
            }
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
        InteractionEvent::AuthStarted { flow_id } => {
            if let Some(page) = ui.input_page.as_mut() {
                page.start_auth(flow_id);
            }
        }
        InteractionEvent::AuthPrompt(prompt) => {
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_auth_prompt(prompt);
            }
        }
        InteractionEvent::AuthPromptWithdrawn { flow_id, prompt_id } => {
            if let Some(page) = ui.input_page.as_mut() {
                page.withdraw_auth_prompt(&flow_id, &prompt_id);
            }
        }
        InteractionEvent::AuthNotice(notice) => {
            if let Some(page) = ui.input_page.as_mut() {
                page.apply_auth_notice(notice);
            }
        }
        InteractionEvent::AuthFinished {
            flow_id,
            outcome,
            message,
        } => {
            if let Some(page) = ui.input_page.as_mut() {
                page.finish_auth(&flow_id, outcome, message);
            }
        }
        InteractionEvent::Error { code, message } => {
            return apply_agent_error(&code, &message, state, ui);
        }
        InteractionEvent::SetEditorText { .. } | InteractionEvent::AsapQueue { .. } => {}
    }
    Vec::new()
}

fn restore_macro_prompt(ui: &mut RuntimeUiState<'_>, original: crate::PromptInput) {
    if ui.input.buf.is_empty() {
        ui.input.restore_prompt(original);
    } else {
        ui.queue
            .retain_failed(crate::interaction::PendingPrompt::new(
                original,
                crate::interaction::PromptDelivery::AfterTurn,
            ));
    }
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
        if let Some(owner) = app
            .session
            .macro_execution
            .as_mut()
            .filter(|owner| owner.snapshot_pending)
        {
            owner.snapshot_error = Some(message.into());
            return Vec::new();
        }
    }
    if matches!(
        code,
        "new-failed"
            | "new-input-failed"
            | "input-failed"
            | "image-input-failed"
            | "skill-unknown"
            | "skill-unavailable"
            | "skill-failed"
            | "pi-rpc-prompt"
            | "command-failed"
            | "command-unknown"
            | "command-invalid-result"
            | "no-commands"
            | "command-cancelled"
    ) {
        let recoveries = {
            let mut app = state.lock().unwrap();
            let command_error = matches!(
                code,
                "command-failed"
                    | "command-unknown"
                    | "command-invalid-result"
                    | "no-commands"
                    | "command-cancelled"
            );
            let mut recoveries = Vec::new();
            let admissions = std::mem::take(&mut app.session.macro_admissions);
            for (id, original) in admissions {
                let failed = app.pending_submissions.contains(&id) && (!command_error || app.transcript.get(&id).is_some_and(|node| matches!(&node.item, crate::display::DisplayItem::Card(card) if card.role == crate::display::CardRole::Skill)));
                if failed {
                    recoveries.push(original);
                } else {
                    app.session.macro_admissions.push((id, original));
                }
            }
            if let Some(execution) = app.session.macro_execution.as_mut() {
                if execution.command_pending || execution.awaiting_run || execution.run_observed {
                    execution.command_pending = false;
                    execution.awaiting_run = false;
                    if !execution.recovered {
                        execution.recovered = true;
                        recoveries.push(execution.original.clone());
                    }
                }
            }
            recoveries
        };
        for original in recoveries {
            restore_macro_prompt(ui, original);
        }
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
                let owner = app.session.macro_execution.as_ref().map(|owner| owner.id);
                let failed = if let Some(id) = owner {
                    ui.queue.fail_record(id)
                } else {
                    ui.queue.fail_model(&model.target)
                };
                if let Some(pending) = failed {
                    if pending.new_mode.is_some() {
                        app.restore_new_conversation_input();
                    }
                    if ui.input.buf.is_empty() {
                        ui.input.restore_prompt(pending.recovery_input());
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
        && !(code == "pi-rpc-prompt" && {
            let app = state.lock().unwrap();
            app.session
                .new_conversation
                .as_ref()
                .is_some_and(|draft| !draft.attached)
                && (app.session.macro_draft_input.is_some()
                    || ui
                        .queue
                        .entries()
                        .iter()
                        .any(|pending| pending.new_mode.is_some() && pending.macro_input.is_some()))
        })
    {
        let (restored, macro_restore) = {
            let mut app = state.lock().unwrap();
            let restored = app.restore_new_conversation_input();
            let macro_original = app.session.macro_draft_input.take();
            let macro_restore = macro_original.is_some();
            let recovered = app
                .session
                .macro_execution
                .as_ref()
                .is_some_and(|execution| execution.recovered);
            let restored = if recovered {
                None
            } else {
                macro_original.or(restored)
            };
            if restored.is_some() {
                let language = app.config.language;
                app.set_new_conversation_notice(tr_args(
                    language,
                    "controller.new_failed",
                    &[("message", message.to_owned())],
                ));
            }
            (restored, macro_restore)
        };
        if let Some(prompt) = restored {
            if macro_restore {
                restore_macro_prompt(ui, prompt);
            } else {
                ui.input.restore_prompt(prompt);
                ui.input.multiline = ui.input.buf.contains('\n');
            }
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
        let skill_ids: Vec<_> = pending.iter().filter(|id| {
            app.transcript.get(id).is_some_and(|node| matches!(&node.item,
                crate::display::DisplayItem::Card(card) if card.role == crate::display::CardRole::Skill))
        }).cloned().collect();
        app.pending_submissions = pending
            .into_iter()
            .filter(|id| {
                !skill_ids
                    .iter()
                    .any(|skill_id| id == skill_id || id.0 == format!("{}:message", skill_id.0))
            })
            .collect();
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
