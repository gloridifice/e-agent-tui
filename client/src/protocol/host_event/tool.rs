//! Tool and Code Mode parsing helpers.

use serde_json::Value;

pub(super) fn string(data: &Value, key: &str, fallback: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

pub(super) fn parse(event_type: &str, data: &Value) -> super::HostEventKind {
    use super::{
        content::{content_text, parse_content},
        HostEventKind,
    };
    match event_type {
        "tool/call" => HostEventKind::ToolCall {
            call_id: string(data, "callId", ""),
            name: string(data, "name", "tool"),
            arguments: string(data, "arguments", ""),
        },
        "tool/result" => {
            let result = data
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
                .and_then(|blocks| blocks.first());
            HostEventKind::ToolResult {
                call_id: result
                    .and_then(|block| block.get("toolCallId"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                output: content_text(
                    &parse_content(result.and_then(|block| block.get("content"))),
                    false,
                ),
                is_error: data.get("error").is_some()
                    || result
                        .and_then(|block| block.get("isError"))
                        .and_then(Value::as_bool)
                        == Some(true),
                output_truncated: data.get("dshTuiOutputTrimmed").and_then(Value::as_bool)
                    == Some(true),
            }
        }
        "tool/code-dispatch-start" => HostEventKind::CodeDispatchStart {
            root_call_id: string(data, "rootCallId", ""),
            parent_call_id: string(data, "parentCallId", ""),
            sub_call_id: string(data, "subCallId", ""),
            name: string(data, "name", "tool"),
            arguments: data
                .get("arguments")
                .map(Value::to_string)
                .unwrap_or_default(),
        },
        "tool/code-dispatch" => HostEventKind::CodeDispatchEnd {
            sub_call_id: string(data, "subCallId", ""),
            is_error: data
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        _ => unreachable!("tool parser called for {event_type}"),
    }
}
