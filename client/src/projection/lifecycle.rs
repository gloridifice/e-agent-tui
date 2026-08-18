use crate::{
    display::{DisplayId, DisplayItem, DisplayTone, TranscriptBlock, TranscriptFormat},
    protocol::{HostEvent, HostEventKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LifecycleProjection {
    TurnStart,
    TurnEnd {
        cancelled: bool,
        outcome: Option<DisplayItem>,
    },
    Goal(Option<String>),
    Plan(Option<String>),
    Preset(String),
    SessionState(String),
    Ignore,
}

pub(crate) fn project(event: &HostEvent) -> Option<LifecycleProjection> {
    match &event.kind {
        HostEventKind::TurnStart => Some(LifecycleProjection::TurnStart),
        HostEventKind::TurnEnd {
            reason,
            error_message,
            error_code,
        } => {
            let outcome = match reason.as_deref() {
                Some("aborted") => Some(block(event, "（已中断）", DisplayTone::Dim)),
                Some("error") => {
                    let text = match (error_message, error_code) {
                        (Some(message), Some(code)) => format!("{message} ({code})"),
                        (Some(message), None) => message.clone(),
                        _ => "turn error".into(),
                    };
                    Some(block(event, &text, DisplayTone::Error))
                }
                Some("blocked") => Some(block(event, "（已阻塞）", DisplayTone::Dim)),
                Some("interrupted") => Some(block(event, "（会话异常中断）", DisplayTone::Warning)),
                Some("max-tokens") => Some(block(
                    event,
                    "（达到模型输出 token 上限）",
                    DisplayTone::Warning,
                )),
                _ => None,
            };
            Some(LifecycleProjection::TurnEnd {
                cancelled: matches!(reason.as_deref(), Some("aborted" | "interrupted")),
                outcome,
            })
        }
        HostEventKind::GoalChange { summary } => Some(LifecycleProjection::Goal(
            (!summary.is_empty()).then(|| summary.clone()),
        )),
        HostEventKind::PlanMode { mode } => Some(LifecycleProjection::Plan(
            (!mode.is_empty()).then(|| mode.clone()),
        )),
        HostEventKind::AgentPresetSelected { preset } => {
            Some(LifecycleProjection::Preset(preset.clone()))
        }
        HostEventKind::SessionState { event_type } => {
            Some(LifecycleProjection::SessionState(event_type.clone()))
        }
        HostEventKind::StepStart { .. }
        | HostEventKind::StepEnd { .. }
        | HostEventKind::AuditOnly { .. } => Some(LifecycleProjection::Ignore),
        _ => None,
    }
}

fn block(event: &HostEvent, text: &str, tone: DisplayTone) -> DisplayItem {
    DisplayItem::Block(TranscriptBlock {
        id: event.seq.map_or_else(
            || DisplayId::correlated("turn-outcome", text),
            |seq| DisplayId::event(seq, "turn-outcome"),
        ),
        unit: None,
        content: text.to_owned(),
        format: TranscriptFormat::Plain,
        tone,
        copy_source: text.to_owned(),
        streaming: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_tokens_is_a_public_warning_block() {
        let event = HostEvent::from_value(serde_json::json!({
            "type":"turn/end", "seq":9,
            "data":{"reason":{"kind":"max-tokens"}}
        }));
        let Some(LifecycleProjection::TurnEnd {
            outcome: Some(DisplayItem::Block(block)),
            ..
        }) = project(&event)
        else {
            panic!("outcome")
        };
        assert_eq!(block.tone, DisplayTone::Warning);
    }
}
