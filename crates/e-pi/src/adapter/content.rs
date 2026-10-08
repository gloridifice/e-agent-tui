//! Pi message-content projection helpers.

use e_tui::agent::timeline::{ContentBlock, MessageSource, TimelineFact, TokenUsage};
use serde_json::Value;

fn pi_skill_parts(text: &str) -> Option<(&str, &str, &str)> {
    let rest = text.strip_prefix("<skill name=\"")?;
    let (name, rest) = rest.split_once("\" location=\"")?;
    let (location, rest) = rest.split_once("\">\n")?;
    if name.is_empty() || location.is_empty() {
        return None;
    }
    let (_, suffix) = rest.rsplit_once("\n</skill>")?;
    if !(suffix.is_empty()
        || suffix
            .strip_prefix("\n\n")
            .is_some_and(|arguments| !arguments.is_empty()))
    {
        return None;
    }
    let instructions = &text[..text.len() - suffix.len()];
    Some((
        name,
        instructions,
        suffix.strip_prefix("\n\n").unwrap_or(""),
    ))
}

pub(super) fn pi_skill_name(text: &str) -> Option<&str> {
    pi_skill_parts(text).map(|(name, _, _)| name)
}

pub(super) fn user_facts(message: &Value) -> Vec<TimelineFact> {
    let fact = user_fact(message);
    let TimelineFact::UserMessage { text, .. } = &fact else {
        return vec![fact];
    };
    let Some((_, instructions, prompt)) =
        pi_skill_parts(text).filter(|(_, _, prompt)| !prompt.is_empty())
    else {
        return vec![fact];
    };
    let instructions = instructions.to_owned();
    let prompt = prompt.to_owned();
    let mut skill = fact;
    let mut prompt_content = vec![ContentBlock::Text(prompt.clone())];
    if let TimelineFact::UserMessage { text, content, .. } = &mut skill {
        *text = instructions.clone();
        prompt_content.extend(
            std::mem::take(content)
                .into_iter()
                .filter(|block| !matches!(block, ContentBlock::Text(_))),
        );
        *content = vec![ContentBlock::Text(instructions)];
    }
    vec![
        skill,
        TimelineFact::UserMessage {
            text: prompt.clone(),
            source_kind: Some("user".into()),
            content: prompt_content,
            source: MessageSource {
                kind: Some("user".into()),
                producer: Some("pi".into()),
                ..Default::default()
            },
        },
    ]
}

pub(super) fn user_fact(message: &Value) -> TimelineFact {
    let value = message.get("content").unwrap_or(&Value::Null);
    let text = content_text(value);
    let skill_name = pi_skill_name(&text).map(str::to_owned);
    let content = if value.is_string() {
        vec![ContentBlock::Text(text.clone())]
    } else {
        content_parts(message)
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => Some(ContentBlock::Text(
                    part.get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                )),
                Some("image") => Some(ContentBlock::Image {
                    label: "image".into(),
                }),
                _ => None,
            })
            .collect()
    };
    TimelineFact::UserMessage {
        text,
        source_kind: Some(if skill_name.is_some() {
            "skill-invocation".into()
        } else {
            "user".into()
        }),
        content,
        source: MessageSource {
            kind: Some(if skill_name.is_some() {
                "skill-invocation".into()
            } else {
                "user".into()
            }),
            form: skill_name.as_ref().map(|_| "instructions".into()),
            summary: skill_name,
            producer: Some("pi".into()),
        },
    }
}
pub(super) fn assistant_fact(
    message: &Value,
    turn: Option<u64>,
    step: Option<u64>,
) -> TimelineFact {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut content = Vec::new();
    for part in content_parts(message) {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                let value = part.get("text").and_then(Value::as_str).unwrap_or("");
                text.push_str(value);
                content.push(ContentBlock::Text(value.to_owned()));
            }
            Some("thinking") => {
                let value = part.get("thinking").and_then(Value::as_str).unwrap_or("");
                reasoning.push_str(value);
                content.push(ContentBlock::Reasoning(value.to_owned()));
            }
            Some("image") => content.push(ContentBlock::Image {
                label: "image".into(),
            }),
            _ => {}
        }
    }
    TimelineFact::AssistantMessage {
        text,
        reasoning,
        content,
        turn,
        step,
        usage: message.get("usage").map(token_usage),
    }
}
pub(super) fn content_parts(message: &Value) -> impl Iterator<Item = &Value> {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}
pub(super) fn content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            (part.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| part.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("")
}
pub(super) fn usage_cost_usd_nanos(message: &Value) -> Option<u64> {
    let usd = message.get("usage")?.get("cost")?.get("total")?.as_f64()?;
    (usd.is_finite() && usd >= 0.0 && usd <= u64::MAX as f64 / 1_000_000_000.0)
        .then(|| (usd * 1_000_000_000.0).round() as u64)
}

pub(super) fn token_usage(value: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: value.get("input").and_then(Value::as_u64).unwrap_or(0),
        output_tokens: value.get("output").and_then(Value::as_u64).unwrap_or(0),
        cache_read_tokens: value.get("cacheRead").and_then(Value::as_u64).unwrap_or(0),
        cache_write_tokens: value.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0),
    }
}
