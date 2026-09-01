use serde_json::Value;

use super::{lifecycle, HostContentBlock};

pub(super) fn parse_content(content: Option<&Value>) -> Vec<HostContentBlock> {
    content
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .map(|block| match block.get("type").and_then(Value::as_str) {
                    Some("text") => HostContentBlock::Text(
                        block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    ),
                    Some("reasoning") => HostContentBlock::Reasoning(
                        block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    ),
                    Some("image") => HostContentBlock::Image {
                        label: block
                            .get("attachment")
                            .and_then(|attachment| {
                                attachment
                                    .get("name")
                                    .or_else(|| attachment.get("attachmentId"))
                            })
                            .and_then(Value::as_str)
                            .unwrap_or("image")
                            .to_owned(),
                    },
                    Some(block_type) => HostContentBlock::Other {
                        block_type: block_type.to_owned(),
                    },
                    None => HostContentBlock::Other {
                        block_type: "unknown".into(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn content_text(content: &[HostContentBlock], reasoning: bool) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            HostContentBlock::Text(text) if !reasoning => Some(text.as_str()),
            HostContentBlock::Reasoning(text) if reasoning => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(super) fn parse(event_type: Option<&str>, data: &Value) -> super::HostEventKind {
    use super::{HostEventKind, HostMessageSource};
    match event_type {
        Some("user/message") => {
            let content = parse_content(data.get("content"));
            let source_value = data.get("source").unwrap_or(&Value::Null);
            let source_kind = source_value
                .get("kind")
                .and_then(Value::as_str)
                .map(str::to_owned);
            HostEventKind::UserMessage {
                text: content_text(&content, false),
                source_kind: source_kind.clone(),
                content,
                source: HostMessageSource {
                    kind: source_kind,
                    form: source_value
                        .get("form")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    summary: source_value
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    producer: source_value
                        .get("plugin")
                        .or_else(|| source_value.get("provider"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            }
        }
        Some("session/title") => HostEventKind::SessionTitle {
            title: lifecycle::optional_string(data, "title"),
        },
        Some("todo/write") => HostEventKind::TodoWrite {
            todos: data
                .get("todos")
                .and_then(Value::as_array)
                .map(|todos| {
                    todos
                        .iter()
                        .filter_map(|todo| {
                            Some((
                                todo.get("content")?.as_str()?.to_owned(),
                                todo.get("status")?.as_str()?.to_owned(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        Some("goal/change") => HostEventKind::GoalChange {
            summary: data
                .get("goal")
                .or_else(|| data.get("summary"))
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                })
                .unwrap_or_default(),
        },
        Some("plan/mode") => HostEventKind::PlanMode {
            mode: data
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        },
        Some("agent-preset/selected") => HostEventKind::AgentPresetSelected {
            preset: data
                .get("agentPreset")
                .or_else(|| data.get("preset"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        },
        Some(
            event_type @ ("request/context" | "permission/preset" | "sandbox/mode"
            | "schedule/change"),
        ) => HostEventKind::SessionState {
            event_type: event_type.to_owned(),
        },
        Some(
            event_type @ ("request/header"
            | "session/end-seed"
            | "subagent/descriptor"
            | "session/title-llm-request"
            | "web/deepseek-search-llm-request"
            | "approval/asked"
            | "approval/decided"
            | "approval/policy"
            | "feedback/record"
            | "agent/inbox/spliced"),
        ) => HostEventKind::AuditOnly {
            event_type: event_type.to_owned(),
        },
        _ => HostEventKind::Unknown {
            event_type: event_type.map(|event_type| event_type.chars().take(160).collect()),
        },
    }
}
