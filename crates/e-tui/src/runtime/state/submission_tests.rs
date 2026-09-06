use super::*;
use crate::{
    agent::timeline::{MessageSource, TimelineFact},
    display::CardRole,
    PromptInput,
};

fn echo(sequence: u64, text: &str, skill: Option<&str>) -> TimelineRecord {
    TimelineRecord {
        sequence: Some(sequence),
        time_ms: None,
        surface: Some(SurfaceOperation::Append),
        source_sequences: Vec::new(),
        fact: TimelineFact::UserMessage {
            text: text.into(),
            source_kind: Some(
                if skill.is_some() {
                    "skill-invocation"
                } else {
                    "user"
                }
                .into(),
            ),
            content: Vec::new(),
            source: MessageSource {
                summary: skill.map(str::to_owned),
                ..Default::default()
            },
        },
    }
}

#[test]
fn submission_is_visible_and_working_before_echo_then_reconciles_once() {
    let mut state = RuntimeState::default();
    for sequence in 1..=2 {
        state.admit_submission(&PromptInput::text("hello"), true);
        assert!(state.session.working);
        assert!(state.session.activity_epoch.is_some());
        let cards = state
            .transcript
            .nodes()
            .iter()
            .filter(|node| matches!(node.item, DisplayItem::Card(_)))
            .count();
        assert_eq!(cards, sequence as usize);
        state.apply_host_event(&echo(sequence, "hello", None));
        assert!(state.pending_submissions.is_empty());
        assert!(
            state.session.working,
            "an idle-status echo must not end pending work"
        );
        let cards = state
            .transcript
            .nodes()
            .iter()
            .filter(|node| matches!(node.item, DisplayItem::Card(_)))
            .count();
        assert_eq!(cards, sequence as usize);
        state.stop_thinking();
    }
}

#[test]
fn skill_echo_replaces_local_card_before_thinking_and_keeps_full_copy_source() {
    let mut state = RuntimeState::default();
    state.admit_submission(&PromptInput::text("/skill:review"), true);
    assert!(
        matches!(&state.transcript.nodes()[0].item, DisplayItem::Card(card)
        if card.role == CardRole::Skill && card.content == "review")
    );
    state.apply_host_event(&echo(1, "full skill instructions", Some("review")));
    assert_eq!(state.transcript.len(), 2);
    let DisplayItem::Card(card) = &state.transcript.nodes()[0].item else {
        panic!("skill first")
    };
    assert_eq!(card.copy_source, "full skill instructions");
    assert_eq!(
        state
            .render
            .units
            .get(&card.unit.unwrap())
            .map(String::as_str),
        Some("full skill instructions")
    );
    assert!(
        matches!(&state.transcript.nodes()[1].item, DisplayItem::Thinking(node) if node.row.state.is_active())
    );
    assert!(state.pending_submissions.is_empty());
    assert!(state.preview_refs.contains_key(&card.id));
}

#[test]
fn non_optimistic_skill_echo_is_inserted_before_running_thinking() {
    let mut state = RuntimeState::default();
    state.session.status = crate::SessionStatus::Running;
    state.start_thinking();
    state.apply_host_event(&echo(1, "instructions", Some("review")));
    assert!(
        matches!(&state.transcript.nodes()[0].item, DisplayItem::Card(card) if card.role == CardRole::Skill)
    );
    assert!(matches!(
        &state.transcript.nodes()[1].item,
        DisplayItem::Thinking(_)
    ));
}

#[test]
fn first_skill_in_draft_has_immediate_card_and_can_be_restored_on_failure() {
    let mut state = RuntimeState::default();
    state.begin_new_conversation("standard");
    let prompt = PromptInput::text("/skill:review");
    assert!(matches!(
        state.materialize_new_conversation(prompt.clone()),
        Some(AgentRequest::NewInput { .. })
    ));
    let draft = state.session.new_conversation.as_ref().unwrap();
    assert_eq!(draft.pending_card.as_ref().unwrap().role, CardRole::Skill);
    assert!(crate::runtime::state::animation_active(
        &state,
        std::time::Instant::now()
    ));
    assert!(state
        .materialize_new_conversation(PromptInput::text("second"))
        .is_none());
    assert_eq!(state.restore_new_conversation_input(), Some(prompt));
    assert!(state
        .session
        .new_conversation
        .as_ref()
        .unwrap()
        .pending_card
        .is_none());
    assert!(!crate::runtime::state::animation_active(
        &state,
        std::time::Instant::now()
    ));
}
