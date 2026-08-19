use crate::{
    agent::timeline::{TimelineFact, TimelineRecord},
    display::{ActivityRow, ActivityState, DisplayId},
};

use super::activity::ActivityMutation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandProjection {
    Command {
        key: String,
        mutation: ActivityMutation,
    },
    Nested {
        key: String,
        mutation: ActivityMutation,
    },
}

pub fn project(
    event: &TimelineRecord,
    parent_id: Option<DisplayId>,
    parent_depth: u16,
) -> Option<CommandProjection> {
    match &event.fact {
        TimelineFact::CommandStarted { id, name, args } => {
            let command_id = id;
            let id = DisplayId::correlated("command", command_id);
            let mut row = ActivityRow::root(id, format!("/{name}"));
            row.summary = args
                .clone()
                .unwrap_or_default()
                .trim()
                .chars()
                .take(120)
                .collect();
            row.start_ms = event.time_ms;
            Some(CommandProjection::Command {
                key: command_id.clone(),
                mutation: ActivityMutation::Upsert(row),
            })
        }
        TimelineFact::CommandFinished { id, success, text } => {
            let command_id = id;
            Some(CommandProjection::Command {
                key: command_id.clone(),
                mutation: ActivityMutation::Settle {
                    id: DisplayId::correlated("command", command_id),
                    state: if *success {
                        ActivityState::Success
                    } else {
                        ActivityState::Failure
                    },
                    summary: text.clone(),
                },
            })
        }
        TimelineFact::SubagentStarted {
            root_id,
            parent_id: parent_call_id,
            id: sub_call_id,
            name,
            summary,
        } => {
            let parent = parent_id.unwrap_or_else(|| {
                DisplayId::correlated(
                    "tool-call",
                    if parent_call_id.is_empty() {
                        root_id
                    } else {
                        parent_call_id
                    },
                )
            });
            let mut row = ActivityRow::root(
                DisplayId::correlated("code-dispatch", sub_call_id),
                name.clone(),
            );
            row.parent_id = Some(parent);
            row.depth = parent_depth.saturating_add(1).max(1);
            row.summary = summary.chars().take(120).collect();
            row.start_ms = event.time_ms;
            Some(CommandProjection::Nested {
                key: sub_call_id.clone(),
                mutation: ActivityMutation::Upsert(row),
            })
        }
        TimelineFact::SubagentFinished { id, failed } => {
            let sub_call_id = id;
            Some(CommandProjection::Nested {
                key: sub_call_id.clone(),
                mutation: ActivityMutation::Settle {
                    id: DisplayId::correlated("code-dispatch", sub_call_id),
                    state: if *failed {
                        ActivityState::Failure
                    } else {
                        ActivityState::Success
                    },
                    summary: None,
                },
            })
        }
        _ => None,
    }
}
