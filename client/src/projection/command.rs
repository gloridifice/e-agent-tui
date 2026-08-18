use crate::{
    display::{ActivityRow, ActivityState, DisplayId},
    protocol::{HostEvent, HostEventKind},
};

use super::activity::ActivityMutation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandProjection {
    Command {
        key: String,
        mutation: ActivityMutation,
    },
    Nested {
        key: String,
        mutation: ActivityMutation,
    },
}

pub(crate) fn project(
    event: &HostEvent,
    parent_id: Option<DisplayId>,
    parent_depth: u16,
) -> Option<CommandProjection> {
    match &event.kind {
        HostEventKind::CommandRun {
            command_id,
            name,
            args,
        } => {
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
        HostEventKind::CommandDone {
            command_id,
            success,
            text,
        } => Some(CommandProjection::Command {
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
        }),
        HostEventKind::CodeDispatchStart {
            root_call_id,
            parent_call_id,
            sub_call_id,
            name,
            arguments,
        } => {
            let parent = parent_id.unwrap_or_else(|| {
                DisplayId::correlated(
                    "tool-call",
                    if parent_call_id.is_empty() {
                        root_call_id
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
            row.summary = arguments.chars().take(120).collect();
            row.start_ms = event.time_ms;
            Some(CommandProjection::Nested {
                key: sub_call_id.clone(),
                mutation: ActivityMutation::Upsert(row),
            })
        }
        HostEventKind::CodeDispatchEnd {
            sub_call_id,
            is_error,
        } => Some(CommandProjection::Nested {
            key: sub_call_id.clone(),
            mutation: ActivityMutation::Settle {
                id: DisplayId::correlated("code-dispatch", sub_call_id),
                state: if *is_error {
                    ActivityState::Failure
                } else {
                    ActivityState::Success
                },
                summary: None,
            },
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_dispatch_keeps_typed_parent_and_depth() {
        let event = HostEvent::from_value(serde_json::json!({
            "type":"tool/code-dispatch-start", "time":5,
            "data":{
                "rootCallId":"root", "parentCallId":"parent", "subCallId":"child",
                "name":"read", "arguments":{"path":"a"}
            }
        }));
        let parent = DisplayId::correlated("tool-call", "parent");
        let Some(CommandProjection::Nested {
            mutation: ActivityMutation::Upsert(row),
            ..
        }) = project(&event, Some(parent.clone()), 2)
        else {
            panic!("nested")
        };
        assert_eq!(row.parent_id, Some(parent));
        assert_eq!(row.depth, 3);
    }
}
