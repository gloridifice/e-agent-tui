//! User/assistant/content projection into public transcript surfaces.

use crate::{
    agent::timeline::{ContentBlock, SurfaceOperation, TimelineFact, TimelineRecord},
    display::{
        CardRole, ContentCard, DisplayId, DisplayItem, DisplayTone, TranscriptBlock,
        TranscriptFormat,
    },
    i18n::{tr, tr_args, Language},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantMutation {
    Append(DisplayItem),
    UpsertReasoning(TranscriptBlock),
    UpsertAnswer(TranscriptBlock),
}

pub fn project(
    event: &TimelineRecord,
    horizontal_padding: usize,
) -> Option<Vec<AssistantMutation>> {
    project_with_language(event, horizontal_padding, Language::English)
}

pub fn project_with_language(
    event: &TimelineRecord,
    horizontal_padding: usize,
    language: Language,
) -> Option<Vec<AssistantMutation>> {
    match &event.fact {
        TimelineFact::UserMessage {
            text,
            source_kind,
            content,
            source,
        } => {
            let attachments = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Image { label } => Some(tr_args(
                        language,
                        "transcript.image",
                        &[("name", label.clone())],
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let mut displayed = text.clone();
            if !attachments.is_empty() {
                if !displayed.is_empty() {
                    displayed.push('\n');
                }
                displayed.push_str(&attachments.join("\n"));
            }
            let id = |role| {
                event.sequence.map_or_else(
                    || DisplayId::correlated(role, "legacy"),
                    |seq| DisplayId::event(seq, role),
                )
            };
            let is_compaction_checkpoint =
                matches!(event.surface, Some(SurfaceOperation::Replace { .. }))
                    && matches!(source.producer.as_deref(), Some("compact" | "compaction"));
            let item = if source_kind.as_deref() == Some("user") && attachments.is_empty() {
                DisplayItem::Card(ContentCard {
                    id: id("user"),
                    unit: None,
                    header: None,
                    content: text.clone(),
                    role: CardRole::User,
                    tone: DisplayTone::Normal,
                    horizontal_padding,
                    copy_source: text.clone(),
                })
            } else if is_compaction_checkpoint {
                DisplayItem::Card(ContentCard {
                    id: id("compaction-summary"),
                    unit: None,
                    header: Some(tr(language, "transcript.compaction_summary")),
                    content: displayed.clone(),
                    role: CardRole::Detail,
                    tone: DisplayTone::Dim,
                    horizontal_padding,
                    copy_source: displayed,
                })
            } else if source.form.as_deref() == Some("notice") {
                let notice = source.summary.as_deref().unwrap_or(&displayed);
                DisplayItem::Block(TranscriptBlock {
                    id: id("notice"),
                    unit: None,
                    content: notice.chars().take(200).collect(),
                    format: TranscriptFormat::Plain,
                    tone: DisplayTone::Dim,
                    copy_source: notice.to_owned(),
                    streaming: false,
                })
            } else if source.form.is_some() || !attachments.is_empty() {
                DisplayItem::Card(ContentCard {
                    id: id("context"),
                    unit: None,
                    header: source.form.as_ref().map(|form| {
                        tr_args(language, "transcript.context", &[("form", form.clone())])
                    }),
                    content: displayed.clone(),
                    role: if source_kind.as_deref() == Some("user") {
                        CardRole::Attachment
                    } else {
                        CardRole::Context
                    },
                    tone: DisplayTone::Dim,
                    horizontal_padding,
                    copy_source: displayed,
                })
            } else {
                DisplayItem::Block(TranscriptBlock {
                    id: id("context-fallback"),
                    unit: None,
                    content: text.chars().take(200).collect(),
                    format: TranscriptFormat::Plain,
                    tone: DisplayTone::Dim,
                    copy_source: text.clone(),
                    streaming: false,
                })
            };
            Some(vec![AssistantMutation::Append(item)])
        }
        TimelineFact::AssistantChunk {
            text,
            reasoning,
            turn,
            step,
            ..
        } => {
            let mut mutations = Vec::new();
            if !reasoning.is_empty() {
                mutations.push(AssistantMutation::UpsertReasoning(reasoning_block(
                    *turn, *step, reasoning, true,
                )));
            }
            if !text.is_empty() {
                mutations.push(AssistantMutation::UpsertAnswer(answer_block(
                    *turn, *step, text, true,
                )));
            }
            Some(mutations)
        }
        TimelineFact::AssistantMessage {
            text,
            reasoning,
            turn,
            step,
            ..
        } => {
            let mut mutations = Vec::new();
            if !reasoning.is_empty() {
                mutations.push(AssistantMutation::UpsertReasoning(reasoning_block(
                    *turn, *step, reasoning, false,
                )));
            }
            if !text.is_empty() {
                mutations.push(AssistantMutation::UpsertAnswer(answer_block(
                    *turn, *step, text, false,
                )));
            }
            Some(mutations)
        }
        _ => None,
    }
}

fn correlation(turn: Option<u64>, step: Option<u64>) -> String {
    format!("{}:{}", turn.unwrap_or(0), step.unwrap_or(0))
}

fn reasoning_block(
    turn: Option<u64>,
    step: Option<u64>,
    text: &str,
    streaming: bool,
) -> TranscriptBlock {
    TranscriptBlock {
        id: DisplayId::correlated("assistant-reasoning", &correlation(turn, step)),
        unit: None,
        content: text.to_owned(),
        format: TranscriptFormat::Reasoning,
        tone: DisplayTone::Dim,
        copy_source: text.to_owned(),
        streaming,
    }
}

fn answer_block(
    turn: Option<u64>,
    step: Option<u64>,
    text: &str,
    streaming: bool,
) -> TranscriptBlock {
    TranscriptBlock {
        id: DisplayId::correlated("assistant-answer", &correlation(turn, step)),
        unit: None,
        content: text.to_owned(),
        format: TranscriptFormat::Markdown,
        tone: DisplayTone::Normal,
        copy_source: text.to_owned(),
        streaming,
    }
}
