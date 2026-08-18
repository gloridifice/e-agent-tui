use serde_json::Value;

use super::{
    content::{content_text, parse_content},
    HostEventKind, TokenUsage,
};

pub(super) fn parse_usage(value: Option<&Value>) -> Option<TokenUsage> {
    let usage = value?;
    Some(TokenUsage {
        input_tokens: usage
            .get("inputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("outputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: usage
            .get("cacheReadTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write_tokens: usage
            .get("cacheWriteTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

pub(super) fn parse(event_type: &str, data: &Value) -> HostEventKind {
    match event_type {
        "assistant/chunk" => {
            let chunk = data.get("chunk").unwrap_or(&Value::Null);
            let chunk_type = chunk.get("type").and_then(Value::as_str);
            HostEventKind::AssistantChunk {
                text: if chunk_type == Some("text-delta") {
                    chunk
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                } else {
                    String::new()
                },
                reasoning: if chunk_type == Some("reasoning-delta") {
                    chunk
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                } else {
                    String::new()
                },
                turn: data.get("turn").and_then(Value::as_u64),
                step: data.get("step").and_then(Value::as_u64),
                usage: (chunk_type == Some("usage"))
                    .then(|| parse_usage(chunk.get("usage")))
                    .flatten(),
            }
        }
        "assistant/message" => {
            let content = parse_content(
                data.get("message")
                    .and_then(|message| message.get("content")),
            );
            HostEventKind::AssistantMessage {
                text: content_text(&content, false),
                reasoning: content_text(&content, true),
                content,
                turn: data.get("turn").and_then(Value::as_u64),
                step: data.get("step").and_then(Value::as_u64),
                usage: parse_usage(data.get("usage")),
            }
        }
        _ => unreachable!("assistant parser called for {event_type}"),
    }
}
