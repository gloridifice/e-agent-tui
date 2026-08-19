use crate::{
    agent::timeline::{TimelineFact, TimelineRecord},
    display::{ActivityRow, ActivityState, DisplayId},
};

use super::activity::ActivityMutation;

pub fn project(
    event: &TimelineRecord,
    existing_summary: Option<String>,
) -> Option<(String, ActivityMutation)> {
    match &event.fact {
        TimelineFact::RetryScheduled {
            id: retry_id,
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
        TimelineFact::RetryStarted {
            id: retry_id,
            retry,
        } => {
            let id = DisplayId::correlated("retry", retry_id);
            let mut row = ActivityRow::root(id, "retry");
            row.state = ActivityState::Running;
            row.summary = existing_summary.unwrap_or_else(|| format!("attempt {retry}"));
            Some((retry_id.clone(), ActivityMutation::Upsert(row)))
        }
        _ => None,
    }
}
