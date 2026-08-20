//! Frontend application root and lifecycle-owned state.
//!
//! `TuiApp` is the synchronous ownership boundary used by the executable.
//! During the staged migration, reducers may still be supplied by the host;
//! the state guard nevertheless contains this single frontend root and every
//! returned action owns its complete effect payload.

use std::time::Instant;

use ratatui::style::Color;

use crate::{
    action::{UiAction, UpdateResult},
    agent::{timeline::TokenUsage, AgentEvent},
    catalog::CatalogModel,
    config::Config,
    display::{DisplayItem, TranscriptFormat},
    event::InputEvent,
    input::InputState,
    interaction::{InteractionModel, ScrollState},
    preview::{
        PreviewContent, PreviewKey, PreviewPaneState, PreviewPolicy, PreviewRef, PreviewRevision,
        PreviewTarget,
    },
    projection::TimelineModel,
    reading::{ReadingDirection, ReadingDocument, ReadingLayout, ReadingViewState},
    render_state::RenderState,
    theme::Theme,
};

pub const BREATH_CYCLE_MS: u128 = 1600;
pub const SETTLE_TRANSITION_MS: u128 = 500;

pub fn lerp_color(from: Color, to: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let lerp =
                |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
            Color::Rgb(lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }
        _ => to,
    }
}

pub fn breathing_color(theme: &Theme, phase: f64) -> Color {
    let t = (1.0 - (phase * std::f64::consts::TAU).cos()) / 2.0;
    lerp_color(
        theme.working_status.idle.fg,
        theme.working_status.running.fg,
        t,
    )
}

pub fn settle_color(from: Color, to: Color, elapsed: std::time::Duration) -> Color {
    if elapsed.as_millis() >= SETTLE_TRANSITION_MS {
        return to;
    }
    lerp_color(
        from,
        to,
        elapsed.as_millis() as f64 / SETTLE_TRANSITION_MS as f64,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionStatus {
    #[default]
    Idle,
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewConversationDraft {
    pub mode: String,
    /// The first prompt remains owned until a real attached-session event
    /// commits the new session.
    pub pending_input: Option<String>,
    pub notice: Option<String>,
}

/// State whose lifetime is the currently attached (or deferred-new) session.
#[derive(Debug)]
pub struct SessionModel {
    pub session_id: Option<String>,
    pub new_conversation: Option<NewConversationDraft>,
    pub status: SessionStatus,
    pub active_commands: usize,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub current_mode: Option<String>,
    pub current_mode_seq: Option<u64>,
    pub token_usage: TokenUsage,
    pub last_usage_sample: Option<(Option<u64>, Option<u64>, TokenUsage)>,
    pub session_title: Option<String>,
    pub session_cwd: Option<String>,
    pub snapshot_truncated: bool,
    pub min_seq: Option<u64>,
    pub history_loading: bool,
    pub history_exhausted: bool,
    pub working: bool,
    pub activity_epoch: Option<Instant>,
}

impl Default for SessionModel {
    fn default() -> Self {
        Self {
            session_id: None,
            new_conversation: None,
            status: SessionStatus::Idle,
            active_commands: 0,
            provider: None,
            model: None,
            current_mode: None,
            current_mode_seq: None,
            token_usage: TokenUsage::default(),
            last_usage_sample: None,
            session_title: None,
            session_cwd: None,
            snapshot_truncated: false,
            min_seq: None,
            history_loading: false,
            history_exhausted: false,
            working: false,
            activity_epoch: None,
        }
    }
}

impl SessionModel {
    pub fn cache_hit_rate(&self) -> Option<u64> {
        let prompt_tokens = self
            .token_usage
            .input_tokens
            .saturating_add(self.token_usage.cache_read_tokens)
            .saturating_add(self.token_usage.cache_write_tokens);
        (prompt_tokens > 0).then(|| {
            self.token_usage
                .cache_read_tokens
                .saturating_mul(100)
                .saturating_add(prompt_tokens / 2)
                / prompt_tokens
        })
    }

    pub fn record_usage(
        &mut self,
        turn: Option<u64>,
        step: Option<u64>,
        usage: Option<TokenUsage>,
    ) {
        let Some(usage) = usage else { return };
        let previous = self
            .last_usage_sample
            .filter(|(old_turn, old_step, _)| *old_turn == turn && *old_step == step)
            .map(|(_, _, usage)| usage)
            .unwrap_or_default();
        self.token_usage.input_tokens = self
            .token_usage
            .input_tokens
            .saturating_sub(previous.input_tokens)
            .saturating_add(usage.input_tokens);
        self.token_usage.output_tokens = self
            .token_usage
            .output_tokens
            .saturating_sub(previous.output_tokens)
            .saturating_add(usage.output_tokens);
        self.token_usage.cache_read_tokens = self
            .token_usage
            .cache_read_tokens
            .saturating_sub(previous.cache_read_tokens)
            .saturating_add(usage.cache_read_tokens);
        self.token_usage.cache_write_tokens = self
            .token_usage
            .cache_write_tokens
            .saturating_sub(previous.cache_write_tokens)
            .saturating_add(usage.cache_write_tokens);
        self.last_usage_sample = Some((turn, step, usage));
    }
}

/// Kernel-neutral frontend application root.
///
/// Additional lifecycle models are introduced here one at a time. Keeping the
/// canonical timeline under this root lets the executable retain temporary
/// forwarding methods without creating a second transcript store.
#[derive(Default)]
pub struct TuiApp {
    pub config: Config,
    pub session: SessionModel,
    pub timeline: TimelineModel,
    pub catalogs: CatalogModel,
    pub interaction: InteractionModel,
    pub render: RenderState,
    pub preview: PreviewPaneState,
    pub reading_document: ReadingDocument,
    pub reading_layout: ReadingLayout,
    pub reading: Option<ReadingViewState>,
    pending_actions: Vec<UiAction>,
}

impl TuiApp {
    pub fn theme(&self) -> Theme {
        self.config.theme()
    }

    pub fn cache_hit_rate(&self) -> Option<u64> {
        self.session.cache_hit_rate()
    }

    pub fn breath_phase(&self) -> f64 {
        match self.session.activity_epoch {
            Some(epoch) => {
                let elapsed = epoch.elapsed().as_millis() % BREATH_CYCLE_MS;
                elapsed as f64 / BREATH_CYCLE_MS as f64
            }
            None => 0.0,
        }
    }

    /// Keep normal-mode Preview on the newest eligible canonical display
    /// owner. Reading mode has a cursor-owned policy and is never stolen by
    /// live appends. History prepend preserves the newest identity.
    /// Assistant markdown answers are already rendered in the main pane and
    /// are never previewed. The merged Thinking node previews its reasoning
    /// content while anything has streamed in (an empty, still-breathing
    /// indicator carries nothing worth previewing); reasoning always previews
    /// even when the main transcript collapses it
    /// (`thinking_display = compact`).
    pub fn reconcile_latest_preview(&mut self) {
        if self.hold_draft_preview_empty() {
            return;
        }
        if self.preview.policy != PreviewPolicy::FollowLatestBlock {
            return;
        }
        let candidate = self
            .timeline
            .transcript
            .nodes()
            .iter()
            .rev()
            .find(|node| {
                !matches!(
                    &node.item,
                    DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown
                ) && !matches!(
                    &node.item,
                    DisplayItem::Thinking(node) if node.content.trim().is_empty()
                )
            })
            .map(|node| (node.id().clone(), node.item.clone()));
        let Some((id, item)) = candidate else {
            self.select_preview(None);
            return;
        };
        let revision = PreviewRevision(self.timeline.transcript.generation());
        let reference = self
            .timeline
            .preview_refs
            .get(&id)
            .cloned()
            .unwrap_or_else(|| {
                let content = match item {
                    DisplayItem::Block(block) => match block.format {
                        TranscriptFormat::Markdown => PreviewContent::Markdown(block.copy_source),
                        TranscriptFormat::Reasoning => PreviewContent::Reasoning(block.copy_source),
                        _ => PreviewContent::PlainText(block.copy_source),
                    },
                    DisplayItem::Card(card) => PreviewContent::PlainText(card.copy_source),
                    DisplayItem::Activity(row) => PreviewContent::PlainText(
                        [row.label, row.summary]
                            .into_iter()
                            .filter(|part| !part.is_empty())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    DisplayItem::Thinking(node) => PreviewContent::Reasoning(node.copy_source),
                    DisplayItem::Composite { detail, .. } => {
                        PreviewContent::PlainText(detail.copy_source)
                    }
                };
                PreviewRef::Inline {
                    key: PreviewKey(format!("display:{}", id.0)),
                    revision,
                    content,
                }
            });
        self.select_preview(Some(PreviewTarget {
            id: id.0,
            reference,
        }));
    }

    fn select_preview(&mut self, target: Option<PreviewTarget>) {
        if let Some(request) = self.preview.select(target) {
            self.pending_actions.push(UiAction::ResolvePreview(request));
        }
    }

    /// A `/new` draft page has no transcript of its own: any preview target
    /// would reference the previous session's content, so every preview
    /// reconcile path must hold the pane empty while a draft is pending.
    /// Returns `true` when the draft page forced the pane empty.
    fn hold_draft_preview_empty(&mut self) -> bool {
        if self.session.new_conversation.is_some() {
            self.select_preview(None);
            return true;
        }
        false
    }

    pub fn take_actions(&mut self) -> Vec<UiAction> {
        std::mem::take(&mut self.pending_actions)
    }

    pub fn rebuild_reading_model(&mut self) {
        self.reading_document = ReadingDocument::derive(&self.timeline, &self.render, &self.config);
        self.reading_layout =
            ReadingLayout::derive(&self.timeline, &self.render, &self.reading_document);
        if self
            .reading
            .as_mut()
            .is_some_and(|reading| !reading.reconcile(&self.reading_document))
        {
            self.reading = None;
            self.preview.policy = PreviewPolicy::FollowLatestBlock;
        }
    }

    pub fn enter_reading(
        &mut self,
        input: &InputState,
        scroll: &mut ScrollState,
        viewport_height: usize,
    ) -> bool {
        self.rebuild_reading_model();
        let viewport = scroll.offset..scroll.offset.saturating_add(viewport_height.max(1));
        let Some(reading) = ReadingViewState::enter(
            &self.reading_document,
            &self.reading_layout,
            viewport,
            input,
        ) else {
            return false;
        };
        self.reading = Some(reading);
        self.preview.policy = PreviewPolicy::FollowReadingCursor;
        self.sync_reading_preview();
        self.keep_reading_visible(scroll, viewport_height);
        true
    }

    pub fn exit_reading(&mut self, input: &mut InputState) {
        if let Some(reading) = self.reading.take() {
            *input = reading.saved_input;
        }
        self.preview.policy = PreviewPolicy::FollowLatestBlock;
        self.reconcile_latest_preview();
    }

    pub fn move_reading_block(
        &mut self,
        delta: isize,
        scroll: &mut ScrollState,
        viewport_height: usize,
    ) -> bool {
        let moved = self
            .reading
            .as_mut()
            .is_some_and(|reading| reading.move_block(&self.reading_document, delta));
        if moved {
            self.sync_reading_preview();
            self.keep_reading_visible(scroll, viewport_height);
        }
        moved
    }

    pub fn enter_reading_items(&mut self) -> bool {
        let entered = self.reading.as_mut().is_some_and(|reading| {
            reading.enter_items(&self.reading_document, &self.reading_layout)
        });
        if entered {
            self.sync_reading_preview();
        }
        entered
    }

    pub fn move_reading_item(
        &mut self,
        direction: ReadingDirection,
        scroll: &mut ScrollState,
        viewport_height: usize,
    ) -> bool {
        let moved = self.reading.as_mut().is_some_and(|reading| {
            reading.move_item(direction, &self.reading_document, &self.reading_layout)
        });
        if moved {
            self.sync_reading_preview();
            self.keep_reading_visible(scroll, viewport_height);
        }
        moved
    }

    /// Return true when Esc only left Item mode; false means the caller should
    /// exit Reading View entirely.
    pub fn leave_reading_items(&mut self) -> bool {
        let left = self
            .reading
            .as_mut()
            .is_some_and(ReadingViewState::leave_items);
        if left {
            self.sync_reading_preview();
        }
        left
    }

    pub fn reading_copy_text(&self) -> Option<String> {
        self.reading
            .as_ref()?
            .copy_payload(&self.reading_document)
            .map(|copy| copy.text.clone())
    }

    fn sync_reading_preview(&mut self) {
        if self.hold_draft_preview_empty() {
            return;
        }
        let target = self.reading.as_ref().and_then(|reading| {
            reading
                .current_item(&self.reading_document)
                .and_then(|item| {
                    item.preview.clone().map(|reference| PreviewTarget {
                        id: item.id.0.clone(),
                        reference,
                    })
                })
                .or_else(|| {
                    reading
                        .current(&self.reading_document)
                        .map(|block| PreviewTarget {
                            id: block.id.0.clone(),
                            reference: block.preview.clone(),
                        })
                })
        });
        self.select_preview(target);
    }

    fn keep_reading_visible(&self, scroll: &mut ScrollState, viewport_height: usize) {
        let Some(geometry) = self
            .reading
            .as_ref()
            .and_then(|reading| self.reading_layout.block(&reading.block_cursor))
        else {
            return;
        };
        let height = viewport_height.max(1);
        let top_threshold = scroll.offset.saturating_add(height / 3);
        let bottom_threshold = scroll.offset.saturating_add((height * 2) / 3);
        let page = height.saturating_sub(1).max(1);
        if geometry.rows.start < top_threshold {
            scroll.offset = scroll.offset.saturating_sub(page);
            scroll.follow = false;
        } else if geometry.rows.end > bottom_threshold {
            let max = self
                .render
                .transcript_cache
                .layout
                .total_rows()
                .saturating_sub(height);
            scroll.offset = scroll.offset.saturating_add(page).min(max);
            scroll.follow = false;
        }
    }

    /// Transitional normalized-agent update entry point.
    ///
    /// The reducer is supplied by the still-migrating controller and must
    /// complete synchronously. External effects are represented only by the
    /// owned actions in the returned `UpdateResult`.
    pub fn update_agent(
        &mut self,
        event: AgentEvent,
        reducer: impl FnOnce(&mut Self, AgentEvent) -> UpdateResult,
    ) -> UpdateResult {
        reducer(self, event)
    }

    /// Transitional frontend-input update entry point. Like `update_agent`,
    /// this never performs I/O and returns complete owned actions.
    pub fn handle_input(
        &mut self,
        event: InputEvent,
        reducer: impl FnOnce(&mut Self, InputEvent) -> UpdateResult,
    ) -> UpdateResult {
        reducer(self, event)
    }
}

impl std::ops::Deref for TuiApp {
    type Target = TimelineModel;

    fn deref(&self) -> &Self::Target {
        &self.timeline
    }
}

impl std::ops::DerefMut for TuiApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.timeline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DirtyState, UiAction};

    #[test]
    fn normalized_entry_points_return_owned_results() {
        let mut app = TuiApp::default();
        let result = app.update_agent(
            AgentEvent::Deadline(crate::agent::DeadlineEvent::Frame),
            |_app, _event| UpdateResult {
                actions: vec![UiAction::WriteClipboard("owned".into())],
                dirty: DirtyState::interaction(),
                next_deadline: None,
            },
        );
        assert!(result.dirty.interaction);
        assert!(matches!(
            result.actions.as_slice(),
            [UiAction::WriteClipboard(text)] if text == "owned"
        ));
    }
}
