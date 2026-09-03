//! Turn, retry, command, and compaction parsing helpers.

use serde_json::Value;

pub(super) fn parse(event_type: &str, data: &Value) -> super::HostEventKind {
    use super::{
        content::{content_text, parse_content},
        HostEventKind,
    };
    match event_type {
        "turn/start" => HostEventKind::TurnStart,
        "step/start" => HostEventKind::StepStart {
            turn: data.get("turn").and_then(Value::as_u64),
            step: data.get("step").and_then(Value::as_u64),
        },
        "step/end" => HostEventKind::StepEnd {
            turn: data.get("turn").and_then(Value::as_u64),
            step: data.get("step").and_then(Value::as_u64),
        },
        "turn/end" => {
            let reason_value = data.get("reason").unwrap_or(&Value::Null);
            let error = reason_value.get("error").unwrap_or(&Value::Null);
            HostEventKind::TurnEnd {
                reason: reason_value
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                error_message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                error_code: error.get("code").and_then(Value::as_str).map(str::to_owned),
            }
        }
        "llm/retry" => HostEventKind::LlmRetry {
            retry_id: data
                .get("retryId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            retry: data.get("retry").and_then(Value::as_u64).unwrap_or(0),
            max_retries: data.get("maxRetries").and_then(Value::as_u64),
            delay_ms: data.get("delayMs").and_then(Value::as_u64).unwrap_or(0),
            message: data
                .get("failure")
                .and_then(|failure| failure.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(160)
                .collect(),
        },
        "llm/retry-started" => HostEventKind::LlmRetryStarted {
            retry_id: data
                .get("retryId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            retry: data.get("retry").and_then(Value::as_u64).unwrap_or(0),
        },
        "command/run" => HostEventKind::CommandRun {
            command_id: data
                .get("commandId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            name: data
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_owned(),
            args: data.get("args").and_then(Value::as_str).map(str::to_owned),
        },
        "command/done" => HostEventKind::CommandDone {
            command_id: data
                .get("commandId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            success: data.get("kind").and_then(Value::as_str) == Some("success"),
            text: data
                .get("text")
                .and_then(Value::as_str)
                .map(|text| text.chars().take(200).collect()),
        },
        "compaction/start" => HostEventKind::CompactionStart {
            compaction_id: data
                .get("compactionId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        },
        "compaction/summary" => {
            let content = parse_content(data.get("summary"));
            HostEventKind::CompactionSummary {
                compaction_id: data
                    .get("compactionId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                summary: content_text(&content, false),
            }
        }
        "compaction/end" => HostEventKind::CompactionEnd {
            compaction_id: data
                .get("compactionId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            error: data
                .get("error")
                .and_then(Value::as_str)
                .map(|error| error.chars().take(200).collect()),
        },
        _ => unreachable!("lifecycle parser called for {event_type}"),
    }
}
