use super::*;
use crate::{
    agent::{
        AgentStatus, AttachedSession, CatalogEvent, InteractionEvent, ModelDescriptor,
        ModelProvider, ModelSelection, SessionEvent, TimelineEvent, TimelineFact, TimelineRecord,
    },
    app::TemporaryModelPhase,
    interaction::PromptDelivery,
};

struct Harness(Arc<Mutex<RuntimeState>>);

impl Harness {
    fn new() -> Self {
        let mut app = RuntimeState::default();
        app.session.session_id = Some("session".into());
        app.config.model_marks.toggle('i', "p", "luna");
        app.config.model_marks.toggle('o', "q", "other");
        app.catalogs.model_providers = [("p", "luna"), ("q", "other")]
            .into_iter()
            .map(|(p, m)| ModelProvider {
                id: p.into(),
                name: p.into(),
                models: vec![ModelDescriptor {
                    id: m.into(),
                    name: m.into(),
                    description: None,
                    context_window: None,
                    reasoning: None,
                }],
            })
            .collect();
        let h = Self(Arc::new(Mutex::new(app)));
        h.confirm(Self::original());
        h
    }

    fn original() -> ModelSelection {
        ModelSelection {
            provider: "base-provider".into(),
            model: "base".into(),
            reasoning_effort: Some("high".into()),
        }
    }

    fn event(&self, event: AgentEvent) -> Vec<UiAction> {
        let mut i = std::mem::take(&mut self.0.lock().unwrap().interaction);
        let effects = RuntimeController::apply_agent(
            event,
            &self.0,
            &mut RuntimeUiState {
                scroll: &mut i.scroll,
                input: &mut i.input,
                input_page: &mut i.input_page,
                approval: &mut i.approval,
                question: &mut i.question,
                queue: &mut i.queue,
            },
        );
        self.0.lock().unwrap().interaction = i;
        effects
    }

    fn confirm(&self, current: ModelSelection) {
        let providers = self.0.lock().unwrap().catalogs.model_providers.clone();
        self.event(AgentEvent::Catalog(CatalogEvent::Models {
            providers,
            current: Some(current),
        }));
    }

    fn confirm_target(&self) {
        let target = self
            .0
            .lock()
            .unwrap()
            .session
            .temporary_model
            .as_ref()
            .unwrap()
            .target
            .clone();
        self.confirm(target);
    }

    fn submit(&self, text: &str, after: bool) -> InputHandlerOutcome {
        let mut queue = std::mem::take(&mut self.0.lock().unwrap().interaction.queue);
        let action = if after {
            InputAction::SendAfterTurn(text.into())
        } else {
            InputAction::Send(text.into())
        };
        let outcome = RuntimeController::apply_input_action(action, &self.0, &mut queue);
        self.0.lock().unwrap().interaction.queue = queue;
        outcome
    }

    fn dispatch(&self) -> Vec<UiAction> {
        RuntimeController::dispatch_next_queued(&self.0)
    }

    fn fact(&self, fact: TimelineFact) {
        self.event(AgentEvent::Timeline(TimelineEvent::Append(
            TimelineRecord {
                sequence: None,
                time_ms: None,
                surface: None,
                source_sequences: vec![],
                fact,
            },
        )));
    }

    fn echo(&self, text: &str) {
        self.fact(TimelineFact::UserMessage {
            text: text.into(),
            source_kind: Some("user".into()),
            content: vec![],
            source: Default::default(),
        });
    }

    fn status(&self, status: AgentStatus) {
        self.event(AgentEvent::Session(SessionEvent::Status(status)));
    }

    fn start(&self) {
        assert!(self.submit("//i commit", false).effects.is_empty());
        assert_model(&self.dispatch(), "p", "luna", None);
        assert!(self.dispatch().is_empty());
        self.confirm_target();
        assert!(
            matches!(self.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Input { prompt })] if prompt == &"commit")
        );
        self.status(AgentStatus::Running);
        self.echo("commit");
    }

    fn error(&self, code: &str) -> Vec<UiAction> {
        self.event(AgentEvent::Interaction(InteractionEvent::Error {
            code: code.into(),
            message: "test failure".into(),
        }))
    }
}

fn assert_model(actions: &[UiAction], p: &str, m: &str, effort: Option<&str>) {
    assert!(
        matches!(actions, [UiAction::Agent(AgentRequest::ModelSet { provider, model, reasoning_effort })]
        if provider == p && model == m && reasoning_effort.as_deref() == effort),
        "{actions:?}"
    );
}

#[test]
fn model_default_effort_is_captured_in_queue_and_does_not_replace_restored_effort() {
    let h = Harness::new();
    {
        let mut app = h.0.lock().unwrap();
        app.catalogs.model_providers[0].models[0].reasoning = Some(crate::agent::ModelReasoning {
            efforts: ["low", "high"]
                .into_iter()
                .map(|id| crate::agent::ReasoningEffort {
                    id: id.into(),
                    name: id.into(),
                    description: None,
                })
                .collect(),
            default_effort: None,
        });
        app.config.model_default_efforts.set("p", "luna", "low");
        app.config
            .model_default_efforts
            .set("base-provider", "base", "low");
    }
    h.submit("//i captured", true);
    h.0.lock()
        .unwrap()
        .config
        .model_default_efforts
        .set("p", "luna", "high");
    assert_model(&h.dispatch(), "p", "luna", Some("low"));
    h.confirm_target();
    assert!(
        matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Input { prompt })]
        if prompt == &"captured")
    );
    h.status(AgentStatus::Running);
    h.echo("captured");
    h.status(AgentStatus::Idle);
    assert_model(&h.dispatch(), "base-provider", "base", Some("high"));
    h.confirm(Harness::original());
    assert!(h.0.lock().unwrap().session.temporary_model.is_none());
}

#[test]
fn temporary_model_keeps_steering_and_tool_steps_then_restores_before_after_turn() {
    let h = Harness::new();
    h.start();
    h.submit("next", true);
    h.submit("steer", false);
    assert!(
        matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Steer { prompt })] if prompt == &"steer")
    );
    h.event(AgentEvent::Interaction(InteractionEvent::AsapQueue {
        session_id: "session".into(),
        prompts: vec!["steer".into()],
        operation: Some(crate::agent::AsapQueueOperation::Submit),
        error: None,
    }));
    h.fact(TimelineFact::TurnEnd {
        reason: Some("toolUse".into()),
        error_message: None,
        error_code: None,
    });
    assert!(
        h.dispatch().is_empty(),
        "a model step is not a completed conversation turn"
    );
    h.event(AgentEvent::Interaction(InteractionEvent::AsapQueue {
        session_id: "session".into(),
        prompts: vec![],
        operation: None,
        error: None,
    }));
    h.status(AgentStatus::Idle);
    assert_model(&h.dispatch(), "base-provider", "base", Some("high"));
    assert!(
        h.dispatch().is_empty(),
        "after-turn waits for restoration confirmation"
    );
    h.confirm(Harness::original());
    assert!(h.0.lock().unwrap().session.temporary_model.is_none());
    assert!(
        matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Input { prompt })] if prompt == &"next")
    );
}

#[test]
fn temporary_model_restores_on_interrupt_failure_and_before_new_immediate_input() {
    for failed in [false, true] {
        let h = Harness::new();
        h.start();
        if failed {
            h.error("input-failed");
        }
        h.fact(TimelineFact::TurnEnd {
            reason: Some(if failed { "error" } else { "cancelled" }.into()),
            error_message: None,
            error_code: None,
        });
        h.status(AgentStatus::Idle);
        assert!(h.submit("new immediate", false).effects.is_empty());
        assert_model(&h.dispatch(), "base-provider", "base", Some("high"));
        h.confirm(Harness::original());
        assert!(
            matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Input { prompt })] if prompt == &"new immediate")
        );
    }
}

#[test]
fn temporary_model_preserves_original_across_marked_steering_and_queued_marks() {
    let h = Harness::new();
    h.start();
    h.submit("//o steer", false);
    assert_model(&h.dispatch(), "q", "other", None);
    h.confirm_target();
    assert!(
        matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Steer { prompt })] if prompt == &"steer")
    );
    assert_eq!(
        h.0.lock()
            .unwrap()
            .session
            .temporary_model
            .as_ref()
            .unwrap()
            .original,
        Harness::original()
    );
    let h = Harness::new();
    h.start();
    h.submit("//o later", true);
    h.0.lock()
        .unwrap()
        .config
        .model_marks
        .toggle('o', "p", "luna");
    h.status(AgentStatus::Idle);
    assert_model(&h.dispatch(), "base-provider", "base", Some("high"));
    h.confirm(Harness::original());
    assert_model(&h.dispatch(), "q", "other", None);
    h.confirm_target();
    assert!(
        matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Input { prompt })] if prompt == &"later")
    );
}

#[test]
fn temporary_model_errors_preserve_prompts_and_block_until_restored() {
    for code in [
        "model-failed",
        "pi-rpc-set_model",
        "pi-rpc-set_thinking_level",
    ] {
        let h = Harness::new();
        h.submit("//i commit", true);
        h.dispatch();
        assert_model(&h.error(code), "base-provider", "base", Some("high"));
        assert_eq!(h.0.lock().unwrap().interaction.input.buf, "//i commit");
        assert!(h.dispatch().is_empty());
        h.error(code);
        assert_eq!(
            h.0.lock()
                .unwrap()
                .session
                .temporary_model
                .as_ref()
                .unwrap()
                .phase,
            TemporaryModelPhase::RestoreFailed
        );
        h.submit("later", false);
        assert!(h.dispatch().is_empty());
        h.confirm(Harness::original());
        assert!(
            matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Input { prompt })] if prompt == &"later")
        );
    }
}

#[test]
fn temporary_model_invalid_prefix_and_empty_input_do_not_send() {
    let h = Harness::new();
    for text in ["//i", "//z hello", "//I hello", "//ii hello"] {
        let outcome = h.submit(text, false);
        assert!(outcome.effects.is_empty());
        assert_eq!(outcome.restore_prompt.unwrap().plain_text(), Some(text));
        assert!(h.dispatch().is_empty());
        assert!(h.0.lock().unwrap().session.temporary_model.is_none());
    }
}

#[test]
fn temporary_model_new_draft_materializes_after_selection_and_restores_after_echo() {
    let h = Harness::new();
    h.0.lock()
        .unwrap()
        .interaction
        .queue
        .push("old after-turn".into(), PromptDelivery::AfterTurn);
    h.0.lock().unwrap().begin_new_conversation("standard");
    h.submit("//i opening", false);
    assert_eq!(
        h.0.lock()
            .unwrap()
            .session
            .new_conversation
            .as_ref()
            .unwrap()
            .pending_input
            .as_ref()
            .unwrap(),
        &"opening"
    );
    assert_model(&h.dispatch(), "p", "luna", None);
    h.confirm_target();
    assert!(
        matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::NewInput { prompt, .. })] if prompt == &"opening")
    );
    assert!(
        h.dispatch().is_empty(),
        "do not restore behind a materializing draft"
    );
    h.event(AgentEvent::Session(SessionEvent::Attached(
        AttachedSession {
            protocol_version: None,
            max_frame_bytes: None,
            id: "new".into(),
            status: AgentStatus::Idle,
            provider: Some("p".into()),
            model: Some("luna".into()),
            mode: None,
            title: None,
            workspace: None,
        },
    )));
    assert!(h.dispatch().is_empty());
    h.status(AgentStatus::Running);
    h.echo("opening");
    h.status(AgentStatus::Idle);
    assert_model(&h.dispatch(), "base-provider", "base", Some("high"));
}

#[test]
fn temporary_model_unrelated_attachment_discards_restoration_state() {
    let h = Harness::new();
    h.start();
    h.submit("old pending", true);
    h.event(AgentEvent::Session(SessionEvent::Attached(
        AttachedSession {
            protocol_version: None,
            max_frame_bytes: None,
            id: "resumed".into(),
            status: AgentStatus::Idle,
            provider: Some("q".into()),
            model: Some("other".into()),
            mode: None,
            title: None,
            workspace: None,
        },
    )));
    assert!(h.0.lock().unwrap().session.temporary_model.is_none());
    assert!(h.dispatch().is_empty());
}

#[test]
fn temporary_model_new_failure_and_cancelled_draft_restore_the_original() {
    for cancel in [false, true] {
        let h = Harness::new();
        h.0.lock().unwrap().begin_new_conversation("standard");
        h.submit("//i opening", false);
        h.dispatch();
        if cancel {
            h.0.lock().unwrap().interaction.queue.cancel_asap();
        }
        h.confirm_target();
        if !cancel {
            h.dispatch();
            h.error("new-input-failed");
            assert_eq!(h.0.lock().unwrap().interaction.input.buf, "opening");
        }
        assert_model(&h.dispatch(), "base-provider", "base", Some("high"));
        h.confirm(Harness::original());
        assert!(h
            .0
            .lock()
            .unwrap()
            .session
            .new_conversation
            .as_ref()
            .unwrap()
            .pending_input
            .is_none());
        assert!(h.dispatch().is_empty());
    }
}

#[test]
fn temporary_model_image_only_body_retains_bytes_without_the_modifier() {
    let h = Harness::new();
    let image = crate::PromptImage {
        media_type: "image/png".into(),
        data: vec![3, 2, 1],
        name: None,
    };
    let prompt = PromptInput {
        parts: vec![
            crate::PromptPart::Text("//i ".into()),
            crate::PromptPart::Image(image.clone()),
        ],
    };
    let mut queue = PendingPromptQueue::default();
    assert!(
        RuntimeController::apply_input_action(InputAction::Send(prompt), &h.0, &mut queue)
            .effects
            .is_empty()
    );
    h.0.lock().unwrap().interaction.queue = queue;
    h.dispatch();
    h.confirm_target();
    assert!(
        matches!(h.dispatch().as_slice(), [UiAction::Agent(AgentRequest::Input { prompt })]
        if prompt.parts == vec![crate::PromptPart::Image(image)])
    );
}

#[test]
fn temporary_model_cancelled_selection_restores_without_sending() {
    let h = Harness::new();
    h.submit("//i commit", false);
    h.dispatch();
    h.0.lock().unwrap().interaction.queue.cancel_asap();
    h.confirm_target();
    assert_model(&h.dispatch(), "base-provider", "base", Some("high"));
    h.confirm(Harness::original());
    assert!(h.dispatch().is_empty());
    assert!(h
        .0
        .lock()
        .unwrap()
        .interaction
        .queue
        .entries()
        .iter()
        .all(|p| p.delivery != PromptDelivery::Asap));
}

#[test]
fn status_flash_catalog_changes_in_drafts_leave_content_caches_alone() {
    let h = Harness::new();
    {
        let mut app = h.0.lock().unwrap();
        assert_eq!(app.reveal_deadline(), None);
        app.begin_new_conversation("standard");
        app.render.transcript_cache.valid = true;
        app.preview.take_work_stats();
    }
    let mut selected = Harness::original();
    selected.model = "other".into();
    h.confirm(selected.clone());
    let first_due = h.0.lock().unwrap().reveal_deadline().unwrap();
    h.confirm(selected);
    {
        let mut app = h.0.lock().unwrap();
        assert_eq!(app.reveal_deadline(), Some(first_due));
        assert!(!crate::runtime::animation_active(&app, first_due));
        assert!(app.tick_reveals(first_due));
        assert!(app.tick_reveals(first_due + std::time::Duration::from_millis(600)));
        assert_eq!(app.reveal_deadline(), None);
        assert!(!app.tick_reveals(first_due + std::time::Duration::from_secs(1)));
        let cache = &app.render.transcript_cache;
        assert!(cache.valid);
        assert!(!cache.tail_dirty);
        assert!(cache.dirty_messages.is_empty());
        assert_eq!(cache.reveal_dirty_from, None);
        assert_eq!(
            app.preview.take_work_stats(),
            crate::PreviewWorkStats::default()
        );
        assert!(app
            .session
            .new_conversation
            .as_ref()
            .unwrap()
            .pending_input
            .is_none());
    }
}

#[test]
fn status_flash_attachment_resets_feedback_and_effort_only_changes_schedule() {
    let h = Harness::new();
    let mut selected = Harness::original();
    selected.provider = "p".into();
    selected.model = "luna".into();
    h.confirm(selected.clone());
    assert!(h.0.lock().unwrap().reveal_deadline().is_some());
    for session_id in ["session", "replacement"] {
        h.event(AgentEvent::Session(SessionEvent::Attached(
            AttachedSession {
                protocol_version: None,
                max_frame_bytes: None,
                id: session_id.into(),
                status: AgentStatus::Idle,
                provider: Some("p".into()),
                model: Some("luna".into()),
                mode: None,
                title: None,
                workspace: None,
            },
        )));
        {
            let mut app = h.0.lock().unwrap();
            assert_eq!(app.reveal_deadline(), None);
            app.catalogs.model_providers[0].models[0].reasoning =
                Some(crate::agent::ModelReasoning {
                    efforts: vec![],
                    default_effort: None,
                });
        }
        h.confirm(selected.clone());
        assert_eq!(h.0.lock().unwrap().reveal_deadline(), None);
        selected.reasoning_effort = Some("max".into());
        h.confirm(selected.clone());
        assert!(h.0.lock().unwrap().reveal_deadline().is_some());
        selected.reasoning_effort = Some("high".into());
    }
}
