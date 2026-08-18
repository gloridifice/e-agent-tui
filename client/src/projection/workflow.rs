use crate::{
    display::{ActivityRow, ActivityState, DisplayId},
    protocol::{HostEvent, HostEventKind, HostLifecycleOutcome},
};

use super::activity::ActivityMutation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkflowProjection {
    Workflow {
        key: String,
        mutation: ActivityMutation,
    },
    Compaction {
        key: String,
        mutation: ActivityMutation,
    },
}

pub(crate) fn project(event: &HostEvent) -> Option<WorkflowProjection> {
    match &event.kind {
        HostEventKind::WorkflowRunStart { run_id, name } => {
            let mut row = ActivityRow::root(DisplayId::correlated("workflow", run_id), "workflow");
            row.summary = name.chars().take(120).collect();
            row.start_ms = event.time_ms;
            Some(WorkflowProjection::Workflow {
                key: run_id.clone(),
                mutation: ActivityMutation::Upsert(row),
            })
        }
        HostEventKind::WorkflowAgentStart {
            run_id,
            member_seq,
            label,
        } => {
            let key = format!("{run_id}:{member_seq}");
            let mut row = ActivityRow::root(DisplayId::correlated("workflow-agent", &key), "agent");
            row.summary = label.chars().take(120).collect();
            row.parent_id = Some(DisplayId::correlated("workflow", run_id));
            row.depth = 1;
            row.start_ms = event.time_ms;
            Some(WorkflowProjection::Workflow {
                key,
                mutation: ActivityMutation::Upsert(row),
            })
        }
        HostEventKind::WorkflowAgentEnd {
            run_id,
            member_seq,
            outcome,
        } => Some(WorkflowProjection::Workflow {
            key: format!("{run_id}:{member_seq}"),
            mutation: ActivityMutation::Settle {
                id: DisplayId::correlated("workflow-agent", &format!("{run_id}:{member_seq}")),
                state: outcome_state(*outcome),
                summary: None,
            },
        }),
        HostEventKind::WorkflowRunEnd { run_id, outcome } => Some(WorkflowProjection::Workflow {
            key: run_id.clone(),
            mutation: ActivityMutation::Settle {
                id: DisplayId::correlated("workflow", run_id),
                state: outcome_state(*outcome),
                summary: None,
            },
        }),
        HostEventKind::CompactionStart { compaction_id } => {
            let mut row = ActivityRow::root(
                DisplayId::correlated("compaction", compaction_id),
                "compacting",
            );
            row.start_ms = event.time_ms;
            Some(WorkflowProjection::Compaction {
                key: compaction_id.clone(),
                mutation: ActivityMutation::Upsert(row),
            })
        }
        HostEventKind::CompactionSummary { compaction_id, .. } => {
            Some(WorkflowProjection::Compaction {
                key: compaction_id.clone(),
                mutation: ActivityMutation::Enrich {
                    id: DisplayId::correlated("compaction", compaction_id),
                    summary: "summary ready".into(),
                    start_ms: None,
                },
            })
        }
        HostEventKind::CompactionEnd {
            compaction_id,
            error,
        } => Some(WorkflowProjection::Compaction {
            key: compaction_id.clone(),
            mutation: ActivityMutation::Settle {
                id: DisplayId::correlated("compaction", compaction_id),
                state: if error.is_none() {
                    ActivityState::Success
                } else {
                    ActivityState::Failure
                },
                summary: error.clone(),
            },
        }),
        _ => None,
    }
}

fn outcome_state(outcome: HostLifecycleOutcome) -> ActivityState {
    match outcome {
        HostLifecycleOutcome::Success => ActivityState::Success,
        HostLifecycleOutcome::Failure => ActivityState::Failure,
        HostLifecycleOutcome::Cancelled => ActivityState::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_workflow_is_not_collapsed_into_failure() {
        let event = HostEvent::from_value(serde_json::json!({
            "type":"tool-workflow/run-end",
            "data":{"runId":"run","stopReason":"cancelled"}
        }));
        let Some(WorkflowProjection::Workflow {
            mutation: ActivityMutation::Settle { state, .. },
            ..
        }) = project(&event)
        else {
            panic!("workflow")
        };
        assert_eq!(state, ActivityState::Cancelled);
    }
}
