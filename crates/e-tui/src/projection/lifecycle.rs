use crate::{
    agent::timeline::{TimelineFact, TimelineRecord},
    display::{DisplayId, DisplayItem, DisplayTone, TranscriptBlock, TranscriptFormat},
    i18n::Language,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleProjection {
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

pub fn project(event: &TimelineRecord, language: Language) -> Option<LifecycleProjection> {
    match &event.fact {
        TimelineFact::TurnStart => Some(LifecycleProjection::TurnStart),
        TimelineFact::TurnEnd {
            reason,
            error_message,
            error_code,
        } => {
            let outcome = match reason.as_deref() {
                Some("aborted") => Some(block(
                    event,
                    &crate::i18n::tr(language, "lifecycle.aborted"),
                    DisplayTone::Dim,
                )),
                Some("error") => {
                    let text = match (error_message, error_code) {
                        (Some(message), Some(code)) => format!("{message} ({code})"),
                        (Some(message), None) => message.clone(),
                        _ => crate::i18n::tr(language, "lifecycle.turn_error"),
                    };
                    Some(block(event, &text, DisplayTone::Error))
                }
                Some("blocked") => Some(block(
                    event,
                    &crate::i18n::tr(language, "lifecycle.blocked"),
                    DisplayTone::Dim,
                )),
                Some("interrupted") => Some(block(
                    event,
                    &crate::i18n::tr(language, "lifecycle.interrupted"),
                    DisplayTone::Warning,
                )),
                Some("max-tokens") => Some(block(
                    event,
                    &crate::i18n::tr(language, "lifecycle.max_tokens"),
                    DisplayTone::Warning,
                )),
                _ => None,
            };
            Some(LifecycleProjection::TurnEnd {
                cancelled: matches!(reason.as_deref(), Some("aborted" | "interrupted")),
                outcome,
            })
        }
        TimelineFact::GoalChanged { summary } => Some(LifecycleProjection::Goal(
            (!summary.is_empty()).then(|| summary.clone()),
        )),
        TimelineFact::ModeChanged { mode } => Some(LifecycleProjection::Plan(
            (!mode.is_empty()).then(|| mode.clone()),
        )),
        TimelineFact::PresetSelected { preset } => {
            Some(LifecycleProjection::Preset(preset.clone()))
        }
        TimelineFact::SessionState { state } => {
            Some(LifecycleProjection::SessionState(state.clone()))
        }
        TimelineFact::StepStart { .. }
        | TimelineFact::StepEnd { .. }
        | TimelineFact::Audit { .. } => Some(LifecycleProjection::Ignore),
        _ => None,
    }
}

fn block(event: &TimelineRecord, text: &str, tone: DisplayTone) -> DisplayItem {
    DisplayItem::Block(TranscriptBlock {
        id: event.sequence.map_or_else(
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
