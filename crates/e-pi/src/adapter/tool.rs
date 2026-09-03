//! Pi tool projection and Preview helpers.

use crate::protocol::RpcRecord;
use e_tui::{
    agent::{
        timeline::{TimelineFact, TimelineRecord},
        tool::{ActivityState, ToolActivity, ToolCapability, ToolReference},
    },
    preview::{
        LineSelection, MutationDiff, MutationHunk, ToolMetrics, ToolPreview, ToolPreviewPrimary,
    },
};
use serde_json::Value;

use super::{
    content::{assistant_fact, content_parts, content_text, token_usage, user_fact},
    AdapterOutput, PiAdapter,
};

const GENERIC_JSON_CHARS: usize = 2_000;

pub(super) fn tool_result_fact(message: &Value) -> TimelineFact {
    let is_error = message
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    TimelineFact::ToolResult {
        activity_id: message
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or("pi-tool")
            .to_owned(),
        output: content_text(message.get("content").unwrap_or(&Value::Null)),
        state: if is_error {
            ActivityState::Failure
        } else {
            ActivityState::Success
        },
        output_truncated: message
            .get("details")
            .and_then(|details| details.get("truncation"))
            .is_some_and(|value| !value.is_null()),
        starts_thinking: false,
        mutation_diff: pi_edit_mutation_diff(
            message.get("toolName").and_then(Value::as_str),
            message,
            is_error,
        ),
        mutation_hunks: Vec::new(),
    }
}
pub(super) fn pi_edit_mutation_diff(
    tool_name: Option<&str>,
    result: &Value,
    is_error: bool,
) -> Option<MutationDiff> {
    if is_error || !tool_name.is_some_and(|name| name.eq_ignore_ascii_case("edit")) {
        return None;
    }
    result
        .get("details")
        .and_then(|details| details.get("patch"))
        .and_then(Value::as_str)
        .filter(|patch| !patch.is_empty())
        .map(|patch| MutationDiff {
            path: None,
            source: patch.to_owned(),
        })
}
pub(super) fn tool_activity(id: &str, name: &str, arguments: Value) -> ToolActivity {
    let capability = match name.to_ascii_lowercase().as_str() {
        "read" => ToolCapability::Read,
        "edit" => ToolCapability::Edit,
        "write" => ToolCapability::Create,
        "grep" | "find" | "ls" => ToolCapability::Search,
        "bash" | "powershell" | "command" | "shell" | "sh" | "pwsh" => ToolCapability::Command,
        _ => ToolCapability::Custom {
            namespace: "pi".into(),
            name: name.into(),
        },
    };
    let string = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            arguments
                .get(*key)
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    };
    let path = string(&["path", "file_path", "filePath"]);
    let command = string(&["command", "cmd"]);
    let query = string(&["pattern", "query"]);
    let summary = command
        .clone()
        .or_else(|| path.clone())
        .or_else(|| query.clone())
        .unwrap_or_else(|| bounded_json(&arguments).0);
    let reference = match capability {
        ToolCapability::Read | ToolCapability::Create => {
            path.clone().map(|path| ToolReference::Path { path })
        }
        ToolCapability::Edit => edit_mutation_hunks(&arguments, path.as_deref())
            .map(ToolReference::Hunks)
            .or_else(|| path.clone().map(|path| ToolReference::Path { path })),
        ToolCapability::Command => command
            .clone()
            .map(|command| ToolReference::Command { command }),
        ToolCapability::Search => Some(ToolReference::SearchResult {
            query: query.clone().unwrap_or_default(),
            matches: Vec::new(),
        }),
        _ => None,
    };
    let preview = match capability {
        ToolCapability::Read | ToolCapability::Create => path.map(|path| ToolPreview {
            name: name.into(),
            primary: ToolPreviewPrimary::Location {
                path,
                lines: arguments
                    .get("offset")
                    .and_then(Value::as_u64)
                    .map(|start| LineSelection {
                        start: start as usize,
                        end: arguments
                            .get("limit")
                            .and_then(Value::as_u64)
                            .map(|limit| start.saturating_add(limit).saturating_sub(1) as usize),
                    }),
            },
            secondary: None,
        }),
        ToolCapability::Command => command.map(|command| ToolPreview {
            name: name.into(),
            primary: ToolPreviewPrimary::Command {
                command,
                metrics: ToolMetrics::default(),
            },
            secondary: None,
        }),
        ToolCapability::Search => Some(ToolPreview {
            name: name.into(),
            primary: ToolPreviewPrimary::Search {
                query: query.unwrap_or_default(),
                path,
            },
            secondary: None,
        }),
        ToolCapability::Edit if reference.is_some() => None,
        ToolCapability::Edit
        | ToolCapability::Insert
        | ToolCapability::Replace
        | ToolCapability::View
        | ToolCapability::Generic
        | ToolCapability::Custom { .. } => {
            let (source, truncated) = bounded_json(&arguments);
            Some(ToolPreview {
                name: name.into(),
                primary: ToolPreviewPrimary::Json { source, truncated },
                secondary: None,
            })
        }
    };
    ToolActivity {
        id: id.into(),
        capability,
        label: name.into(),
        summary,
        state: ActivityState::Running,
        reference,
        items: Vec::new(),
        preview,
    }
}
pub(super) fn edit_mutation_hunks(
    arguments: &Value,
    path: Option<&str>,
) -> Option<Vec<MutationHunk>> {
    let replacements = if let Some(edits) = arguments.get("edits").and_then(Value::as_array) {
        if edits.is_empty() {
            return None;
        }
        edits
            .iter()
            .map(|edit| {
                Some((
                    edit.get("oldText")?.as_str()?,
                    edit.get("newText")?.as_str()?,
                ))
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        vec![(
            arguments.get("oldText")?.as_str()?,
            arguments.get("newText")?.as_str()?,
        )]
    };
    Some(
        replacements
            .into_iter()
            .map(|(old, new)| MutationHunk {
                path: path.map(str::to_owned),
                old: Some(old.to_owned()),
                new: Some(new.to_owned()),
                anchor_line: None,
            })
            .collect(),
    )
}
pub(super) fn bounded_json(value: &Value) -> (String, bool) {
    let source = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
    let mut chars = source.chars();
    let bounded = chars.by_ref().take(GENERIC_JSON_CHARS).collect::<String>();
    let truncated = chars.next().is_some();
    (bounded, truncated)
}
pub(super) fn thinking_label(level: &str) -> String {
    let mut chars = level.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// Live tool/message lifecycle projection.

pub(super) fn message_update(adapter: &mut PiAdapter, record: &RpcRecord) -> AdapterOutput {
    let Some(delta) = record.field("assistantMessageEvent") else {
        return AdapterOutput::default();
    };
    let kind = delta.get("type").and_then(Value::as_str);
    let (text, reasoning) = match kind {
        Some("text_delta") => (delta.get("delta").and_then(Value::as_str).unwrap_or(""), ""),
        Some("thinking_delta") => ("", delta.get("delta").and_then(Value::as_str).unwrap_or("")),
        _ => return AdapterOutput::default(),
    };
    adapter.timeline(TimelineFact::AssistantChunk {
        text: text.to_owned(),
        reasoning: reasoning.to_owned(),
        turn: Some(adapter.current_turn.max(1)),
        step: Some(0),
        usage: record.field("usage").map(token_usage),
    })
}

pub(super) fn live_message(adapter: &mut PiAdapter, message: &Value) -> AdapterOutput {
    match message.get("role").and_then(Value::as_str) {
        Some("user") => {
            adapter.note_first_user_title(&content_text(
                message.get("content").unwrap_or(&Value::Null),
            ));
            let mut output = adapter.timeline(user_fact(message));
            output.events.extend(adapter.title_events());
            output
        }
        Some("assistant") => adapter.timeline(assistant_fact(
            message,
            Some(adapter.current_turn.max(1)),
            Some(0),
        )),
        // `tool_execution_end` carries the same result immediately
        // before Pi appends its durable `toolResult` message. The former
        // owns live tool projection; suppress the latter duplicate.
        Some("toolResult") => {
            let id = message
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("pi-tool");
            if adapter.pending_tool_result_messages.remove(id) {
                AdapterOutput::default()
            } else {
                adapter.timeline(tool_result_fact(message))
            }
        }
        _ => AdapterOutput::default(),
    }
}

pub(super) fn snapshot_message(
    adapter: &mut PiAdapter,
    message: &Value,
    turn: Option<u64>,
) -> Vec<TimelineRecord> {
    match message.get("role").and_then(Value::as_str) {
        Some("user") => vec![adapter.record_fact(user_fact(message))],
        Some("assistant") => {
            let mut records = vec![adapter.record_fact(assistant_fact(message, turn, Some(0)))];
            for call in content_parts(message)
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("toolCall"))
            {
                records.push(adapter.record_fact(TimelineFact::ToolCall(tool_activity(
                    call.get("id").and_then(Value::as_str).unwrap_or("pi-tool"),
                    call.get("name").and_then(Value::as_str).unwrap_or("tool"),
                    call.get("arguments").cloned().unwrap_or(Value::Null),
                ))));
            }
            records
        }
        Some("toolResult") => vec![adapter.record_fact(tool_result_fact(message))],
        Some("compactionSummary") => vec![adapter.record_fact(TimelineFact::CompactionSummary {
            id: "pi-compaction".into(),
            summary: message
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        })],
        Some("branchSummary") => vec![adapter.record_fact(TimelineFact::Custom {
            namespace: "pi".into(),
            kind: Some("branch-summary".into()),
            summary: message
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })],
        _ => Vec::new(),
    }
}

pub(super) fn tool_start(adapter: &mut PiAdapter, record: &RpcRecord) -> AdapterOutput {
    let id = record.string("toolCallId").unwrap_or("pi-tool");
    let name = record.string("toolName").unwrap_or("tool");
    let args = record.field("args").cloned().unwrap_or(Value::Null);
    adapter.timeline(TimelineFact::ToolCall(tool_activity(id, name, args)))
}

pub(super) fn tool_end(adapter: &mut PiAdapter, record: &RpcRecord) -> AdapterOutput {
    let result = record.field("result").cloned().unwrap_or(Value::Null);
    let is_error = record.bool("isError").unwrap_or(false);
    let activity_id = record.string("toolCallId").unwrap_or("pi-tool").to_owned();
    adapter
        .pending_tool_result_messages
        .insert(activity_id.clone());
    adapter.timeline(TimelineFact::ToolResult {
        activity_id,
        output: content_text(result.get("content").unwrap_or(&Value::Null)),
        state: if is_error {
            ActivityState::Failure
        } else {
            ActivityState::Success
        },
        output_truncated: result
            .get("details")
            .and_then(|details| details.get("truncation"))
            .is_some_and(|value| !value.is_null()),
        starts_thinking: false,
        mutation_diff: pi_edit_mutation_diff(record.string("toolName"), &result, is_error),
        mutation_hunks: Vec::new(),
    })
}
