use crate::{
    agent::timeline::{LifecycleOutcome, TimelineFact, TimelineRecord},
    display::{ActivityRow, ActivityState, DisplayId},
};

use super::activity::ActivityMutation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowProjection {
    Workflow {
        key: String,
        mutation: ActivityMutation,
    },
    Compaction {
        key: String,
        mutation: ActivityMutation,
    },
}

pub fn project(event: &TimelineRecord) -> Option<WorkflowProjection> {
    match &event.fact {
        TimelineFact::WorkflowStarted { id: run_id, name } => {
            let mut row = ActivityRow::root(DisplayId::correlated("workflow", run_id), "workflow");
            row.summary = name.chars().take(120).collect();
            row.start_ms = event.time_ms;
            Some(WorkflowProjection::Workflow {
                key: run_id.clone(),
                mutation: ActivityMutation::Upsert(row),
            })
        }
        TimelineFact::WorkflowMemberStarted {
            workflow_id: run_id,
            sequence: member_seq,
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
        TimelineFact::WorkflowMemberFinished {
            workflow_id: run_id,
            sequence: member_seq,
            outcome,
        } => Some(WorkflowProjection::Workflow {
            key: format!("{run_id}:{member_seq}"),
            mutation: ActivityMutation::Settle {
                id: DisplayId::correlated("workflow-agent", &format!("{run_id}:{member_seq}")),
                label: None,
                state: outcome_state(*outcome),
                summary: None,
            },
        }),
        TimelineFact::WorkflowFinished {
            id: run_id,
            outcome,
        } => Some(WorkflowProjection::Workflow {
            key: run_id.clone(),
            mutation: ActivityMutation::Settle {
                id: DisplayId::correlated("workflow", run_id),
                label: None,
                state: outcome_state(*outcome),
                summary: None,
            },
        }),
        TimelineFact::CompactionStarted {
            id: compaction_id,
            model_name,
        } => {
            let mut row = ActivityRow::root(
                DisplayId::correlated("compaction", compaction_id),
                compaction_label("compacting", model_name.as_deref()),
            );
            row.start_ms = event.time_ms;
            Some(WorkflowProjection::Compaction {
                key: compaction_id.clone(),
                mutation: ActivityMutation::Upsert(row),
            })
        }
        TimelineFact::CompactionSummary {
            id: compaction_id, ..
        } => Some(WorkflowProjection::Compaction {
            key: compaction_id.clone(),
            mutation: ActivityMutation::Enrich {
                id: DisplayId::correlated("compaction", compaction_id),
                summary: "summary ready".into(),
                start_ms: None,
            },
        }),
        TimelineFact::CompactionFinished {
            id: compaction_id,
            model_name,
            error,
        } => Some(WorkflowProjection::Compaction {
            key: compaction_id.clone(),
            mutation: ActivityMutation::Settle {
                id: DisplayId::correlated("compaction", compaction_id),
                label: error
                    .is_none()
                    .then(|| compaction_label("compacting complete", model_name.as_deref())),
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

fn compaction_label(label: &str, model: Option<&str>) -> String {
    model
        .filter(|name| !name.is_empty())
        .map_or_else(|| label.to_owned(), |name| format!("{label} with {name}"))
}

fn outcome_state(outcome: LifecycleOutcome) -> ActivityState {
    match outcome {
        LifecycleOutcome::Success => ActivityState::Success,
        LifecycleOutcome::Failure => ActivityState::Failure,
        LifecycleOutcome::Cancelled => ActivityState::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_legacy_labels_remain_lowercase_without_inventing_a_model() {
        assert_eq!(compaction_label("compacting", None), "compacting");
        assert_eq!(
            compaction_label("compacting complete", None),
            "compacting complete"
        );
    }
}
