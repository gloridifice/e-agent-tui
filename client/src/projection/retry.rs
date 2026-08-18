use crate::{
    display::{ActivityRow, ActivityState, DisplayId},
    protocol::{HostEvent, HostEventKind},
};

use super::activity::ActivityMutation;

pub(crate) fn project(
    event: &HostEvent,
    existing_summary: Option<String>,
) -> Option<(String, ActivityMutation)> {
    match &event.kind {
        HostEventKind::LlmRetry {
            retry_id,
            retry,
            max_retries,
            delay_ms,
            message,
        } => {
            let id = DisplayId::correlated("retry", retry_id);
            let mut row = ActivityRow::root(id, "retry");
            row.state = ActivityState::Waiting;
            row.start_ms = event.time_ms;
            row.summary = match max_retries {
                Some(max) => format!("{retry}/{max} · {delay_ms}ms · {message}"),
                None => format!("{retry} · {delay_ms}ms · {message}"),
            };
            Some((retry_id.clone(), ActivityMutation::Upsert(row)))
        }
        HostEventKind::LlmRetryStarted { retry_id, retry } => {
            let id = DisplayId::correlated("retry", retry_id);
            let mut row = ActivityRow::root(id, "retry");
            row.state = ActivityState::Running;
            row.summary = existing_summary.unwrap_or_else(|| format!("attempt {retry}"));
            Some((retry_id.clone(), ActivityMutation::Upsert(row)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_and_started_share_identity_and_keep_details() {
        let schedule = HostEvent::from_value(serde_json::json!({
            "type":"llm/retry", "time":10,
            "data":{"retryId":"r","retry":1,"maxRetries":3,"delayMs":50,"message":"busy"}
        }));
        let (_, ActivityMutation::Upsert(waiting)) = project(&schedule, None).unwrap() else {
            panic!("waiting")
        };
        let started = HostEvent::from_value(serde_json::json!({
            "type":"llm/retry-started", "data":{"retryId":"r","retry":1}
        }));
        let (_, ActivityMutation::Upsert(running)) =
            project(&started, Some(waiting.summary.clone())).unwrap()
        else {
            panic!("running")
        };
        assert_eq!(waiting.id, running.id);
        assert_eq!(running.summary, waiting.summary);
        assert_eq!(running.state, ActivityState::Running);
    }
}
