//! Workflow lifecycle parsing helpers.

use serde_json::Value;

use super::HostLifecycleOutcome;

pub(super) fn outcome(value: Option<&Value>) -> HostLifecycleOutcome {
    match value.and_then(Value::as_str) {
        Some("completed") => HostLifecycleOutcome::Success,
        Some("cancelled") => HostLifecycleOutcome::Cancelled,
        _ => HostLifecycleOutcome::Failure,
    }
}

pub(super) fn parse(event_type: &str, data: &Value) -> super::HostEventKind {
    use super::HostEventKind;
    match event_type {
        "tool-workflow/run-start" => HostEventKind::WorkflowRunStart {
            run_id: string(data, "runId", ""),
            name: string(data, "name", "workflow"),
        },
        "tool-workflow/agent-start" => HostEventKind::WorkflowAgentStart {
            run_id: string(data, "runId", ""),
            member_seq: data.get("seq").and_then(Value::as_u64).unwrap_or(0),
            label: string(data, "label", "agent"),
        },
        "tool-workflow/agent-end" => HostEventKind::WorkflowAgentEnd {
            run_id: string(data, "runId", ""),
            member_seq: data.get("seq").and_then(Value::as_u64).unwrap_or(0),
            outcome: outcome(data.get("outcome")),
        },
        "tool-workflow/run-end" => HostEventKind::WorkflowRunEnd {
            run_id: string(data, "runId", ""),
            outcome: outcome(data.get("stopReason")),
        },
        _ => unreachable!("workflow parser called for {event_type}"),
    }
}

fn string(data: &Value, key: &str, fallback: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}
