//! Shared runner scheduling and inbound-admission policy.
//!
//! Executable adapters retain their provider-specific `tokio::select!` loops.
//! This module owns only decisions that must remain identical once an adapter
//! has normalized an inbound value into an [`crate::AgentEvent`].

use std::time::{Duration, Instant};

use crate::{agent::TimelineEvent, AgentEvent};

pub const MIN_ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
pub const INBOUND_BATCH_LIMIT: usize = 64;
pub const INBOUND_BATCH_BUDGET: Duration = Duration::from_millis(2);

/// Wait until `deadline`, or remain pending forever when no work is due.
///
/// The pending branch is deliberate: idle runners must not wake periodically.
pub async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending::<()>().await,
    }
}

/// Clamp a configured spinner cadence to the shared visible-animation minimum.
pub fn animation_interval(configured_millis: u64) -> Duration {
    Duration::from_millis(configured_millis.max(MIN_ANIMATION_INTERVAL.as_millis() as u64))
}

/// Whether another inbound item may be admitted in the current fairness turn.
pub fn inbound_budget_remaining(count: usize, elapsed: Duration) -> bool {
    count < INBOUND_BATCH_LIMIT && elapsed < INBOUND_BATCH_BUDGET
}

/// Whether a normalized event is a live assistant text/reasoning delta that
/// must hand control back to rendering without consuming the batch backlog.
pub fn is_streaming_delta(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::Timeline(TimelineEvent::Append(record))
            if matches!(
                &record.fact,
                crate::agent::TimelineFact::AssistantChunk {
                    text,
                    reasoning,
                    ..
                } if !text.is_empty() || !reasoning.is_empty()
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{TimelineFact, TimelineRecord};

    #[tokio::test]
    async fn idle_deadline_wait_does_not_periodically_wake() {
        assert!(
            tokio::time::timeout(Duration::from_millis(1), wait_for_deadline(None))
                .await
                .is_err()
        );
    }

    #[test]
    fn shared_fairness_limits_admit_only_before_each_budget() {
        assert!(inbound_budget_remaining(
            INBOUND_BATCH_LIMIT - 1,
            INBOUND_BATCH_BUDGET - Duration::from_nanos(1)
        ));
        assert!(!inbound_budget_remaining(
            INBOUND_BATCH_LIMIT,
            Duration::ZERO
        ));
        assert!(!inbound_budget_remaining(0, INBOUND_BATCH_BUDGET));
    }

    #[test]
    fn animation_interval_clamps_to_the_shared_minimum() {
        assert_eq!(animation_interval(1), MIN_ANIMATION_INTERVAL);
        assert_eq!(animation_interval(32), Duration::from_millis(32));
    }

    #[test]
    fn streaming_delta_classification_requires_visible_assistant_content() {
        let chunk = |text: &str, reasoning: &str| {
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                sequence: None,
                time_ms: None,
                surface: None,
                source_sequences: Vec::new(),
                fact: TimelineFact::AssistantChunk {
                    text: text.into(),
                    reasoning: reasoning.into(),
                    turn: None,
                    step: None,
                    usage: None,
                },
            }))
        };
        assert!(is_streaming_delta(&chunk("text", "")));
        assert!(is_streaming_delta(&chunk("", "reasoning")));
        assert!(!is_streaming_delta(&chunk("", "")));
    }
}
