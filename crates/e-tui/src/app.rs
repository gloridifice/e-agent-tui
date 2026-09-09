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
    display::{CardRole, DisplayItem, TranscriptFormat},
    event::InputEvent,
    input::InputState,
    interaction::{InteractionModel, ScrollState},
    preview::{
        PreviewContent, PreviewKey, PreviewPaneState, PreviewPolicy, PreviewRef,
        PreviewRevealIntent, PreviewRevision, PreviewTarget,
    },
    projection::TimelineModel,
    reading::{ReadingDirection, ReadingDocument, ReadingLayout, ReadingViewState},
    render_state::RenderState,
    theme::Theme,
};

pub const BREATH_CYCLE_MS: u128 = 1600;
pub const ACTIVITY_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub const SETTLE_TRANSITION_MS: u128 = 500;

pub fn lerp_color(from: Color, to: Color, t: f64) -> Color {
    crate::color::lerp_rgb(from, to, t)
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
pub enum FrontendKind {
    #[default]
    Dsh,
    Pi,
}

impl FrontendKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dsh => "dsh",
            Self::Pi => "pi",
        }
    }
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
    /// The first prompt remains owned across attachment until its direct user
    /// message commits the new session or admission reports a failure.
    pub pending_input: Option<crate::PromptInput>,
    pub pending_card: Option<crate::display::ContentCard>,
    pub attached: bool,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporaryModelPhase {
    Selecting,
    Ready,
    Active,
    Restoring,
    RestoreFailed,
}

#[derive(Debug, Clone)]
pub struct TemporaryModel {
    pub original: crate::agent::ModelSelection,
    pub target: crate::agent::ModelSelection,
    pub phase: TemporaryModelPhase,
    pub materializing: bool,
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
    pub temporary_model: Option<TemporaryModel>,
    pub current_mode: Option<String>,
    pub current_mode_seq: Option<u64>,
    pub token_usage: TokenUsage,
    pub cost_usd: Option<f64>,
    pub last_usage_sample: Option<(Option<u64>, Option<u64>, TokenUsage)>,
    pub context_usage_unknown: bool,
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
            temporary_model: None,
            current_mode: None,
            current_mode_seq: None,
            token_usage: TokenUsage::default(),
            cost_usd: None,
            last_usage_sample: None,
            context_usage_unknown: false,
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

    pub fn context_usage_percent(&self, context_window: u64) -> u64 {
        if context_window == 0 {
            return 0;
        }
        let usage = self
            .last_usage_sample
            .map(|(_, _, usage)| usage)
            .unwrap_or_default();
        let context_tokens = usage
            .input_tokens
            .saturating_add(usage.output_tokens)
            .saturating_add(usage.cache_read_tokens)
            .saturating_add(usage.cache_write_tokens);
        context_tokens
            .saturating_mul(100)
            .saturating_add(context_window / 2)
            / context_window
    }

    pub fn record_usage(
        &mut self,
        turn: Option<u64>,
        step: Option<u64>,
        usage: Option<TokenUsage>,
    ) {
        let Some(usage) = usage.filter(|usage| *usage != TokenUsage::default()) else {
            return;
        };
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
        self.context_usage_unknown = false;
    }
}

#[derive(Debug)]
struct ReadingLayoutAnchor {
    block: crate::reading::BlockId,
    screen_row: usize,
    viewport_height: usize,
}

/// Kernel-neutral frontend application root.
///
/// Additional lifecycle models are introduced here one at a time. Keeping the
/// canonical timeline under this root lets the executable retain temporary
/// forwarding methods without creating a second transcript store.
#[derive(Default)]
pub struct TuiApp {
    pub frontend: FrontendKind,
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
    pub history_page: Option<crate::history_page::HistoryPage>,
    pub next_history_request_id: u64,
    reading_layout_anchor: Option<ReadingLayoutAnchor>,
    pending_actions: Vec<UiAction>,
}

fn is_normal_preview_eligible(item: &DisplayItem) -> bool {
    match item {
        DisplayItem::Block(block) => !matches!(
            block.format,
            TranscriptFormat::Markdown | TranscriptFormat::Plain
        ),
        DisplayItem::Card(card) => !matches!(card.role, CardRole::User | CardRole::Attachment),
        DisplayItem::Thinking(node) => !node.content.trim().is_empty(),
        DisplayItem::Activity(_) | DisplayItem::Composite { .. } => true,
    }
}

impl TuiApp {
    pub fn theme(&self) -> Theme {
        self.config.theme()
    }

    pub fn cache_hit_rate(&self) -> Option<u64> {
        self.session.cache_hit_rate()
    }

    pub fn transcript_reveal_deadline(&self) -> Option<Instant> {
        self.render
            .transcript_reveals
            .values()
            .filter_map(|track| track.next_due())
            .min()
    }

    pub fn reveal_deadline(&self) -> Option<Instant> {
        match (
            self.transcript_reveal_deadline(),
            self.preview.reveal_deadline(),
        ) {
            (Some(transcript), Some(preview)) => Some(transcript.min(preview)),
            (transcript, preview) => transcript.or(preview),
        }
    }

    /// Advance each due transcript lane by at most one grapheme/fade step.
    /// Returns true when the frame became dirty.
    pub fn tick_transcript_reveals(&mut self, now: Instant) -> bool {
        let rate = self.config.message_chars_per_second.get();
        let ids = self
            .render
            .transcript_reveals
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut changed = false;
        let mut completed = Vec::new();
        for id in ids {
            let Some((track_changed, complete)) = self
                .render
                .transcript_reveals
                .get_mut(&id)
                .map(|track| (track.tick(now, rate), track.is_complete()))
            else {
                continue;
            };
            if track_changed {
                changed = true;
                if let Some(index) = self.transcript.position(&id) {
                    self.render.transcript_cache.mark_reveal_dirty(index);
                }
            }
            if complete {
                completed.push(id);
            }
        }
        for id in completed {
            self.render.transcript_reveals.remove(&id);
        }
        changed
    }

    pub fn tick_reveals(&mut self, now: Instant) -> bool {
        let transcript = self.tick_transcript_reveals(now);
        let preview = self
            .preview
            .tick_reveal(now, self.config.preview_lines_per_second.get());
        transcript || preview
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

    pub fn activity_spinner_frame(&self) -> &'static str {
        ACTIVITY_SPINNER_FRAMES[self.render.activity_frame % ACTIVITY_SPINNER_FRAMES.len()]
    }

    /// Keep normal-mode Preview on the newest eligible canonical display
    /// owner. Reading mode has a cursor-owned policy and is never stolen by
    /// live appends. History prepend preserves the newest identity.
    /// Assistant Markdown, plain system/error output, and user-owned cards are
    /// already complete in the main pane and are never followed automatically.
    /// The merged Thinking node previews its reasoning content while anything
    /// has streamed in (an empty, still-running indicator carries nothing worth
    /// previewing); reasoning always previews even when the main transcript
    /// collapses it (`thinking_display = compact`). Reading View keeps its
    /// explicit selected-block Preview policy.
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
            .find(|node| is_normal_preview_eligible(&node.item))
            .map(|node| (node.id().clone(), node.revision(), node.item.clone()));
        let Some((id, node_revision, item)) = candidate else {
            self.select_preview(None);
            return;
        };
        let revision = PreviewRevision(node_revision);
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
                    DisplayItem::Card(card) => match card.role {
                        CardRole::Skill | CardRole::Context => {
                            PreviewContent::MutedMarkdown(card.copy_source)
                        }
                        _ => PreviewContent::PlainText(card.copy_source),
                    },
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
        let intent = if self.replaying || self.preview.policy == PreviewPolicy::FollowReadingCursor
        {
            PreviewRevealIntent::Page
        } else {
            PreviewRevealIntent::FreshLive
        };
        if let Some(request) = self.preview.select_with_intent(target, intent) {
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
        self.reading_layout_anchor =
            self.reading_layout
                .block(&reading.block_cursor)
                .map(|geometry| ReadingLayoutAnchor {
                    block: reading.block_cursor.clone(),
                    screen_row: geometry.rows.start.saturating_sub(scroll.offset),
                    viewport_height,
                });
        self.reading = Some(reading);
        self.render.transcript_cache.invalidate();
        self.preview.policy = PreviewPolicy::FollowReadingCursor;
        self.sync_reading_preview();
        true
    }

    pub fn exit_reading(&mut self, input: &mut InputState) {
        self.reading_layout_anchor = None;
        if let Some(reading) = self.reading.take() {
            *input = reading.saved_input;
            self.render.transcript_cache.invalidate();
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

    pub(crate) fn clear_reading_layout_anchor(&mut self) {
        self.reading_layout_anchor = None;
    }

    pub(crate) fn reconcile_reading_layout_anchor(&mut self, scroll: &mut ScrollState) {
        let Some(anchor) = self.reading_layout_anchor.take() else {
            return;
        };
        if self
            .reading
            .as_ref()
            .is_some_and(|reading| reading.block_cursor == anchor.block)
        {
            if let Some(geometry) = self.reading_layout.block(&anchor.block) {
                scroll.offset = geometry.rows.start.saturating_sub(anchor.screen_row);
            }
        }
        self.keep_reading_visible(scroll, anchor.viewport_height);
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
        // Use the ceiling quarter as a safe margin. A floored fractional row
        // can place the cursor just across the opposite page threshold and
        // make the next reconciliation jump straight back.
        let margin = height.div_ceil(4);
        let top_threshold = scroll.offset.saturating_add(margin);
        let bottom_threshold = scroll.offset.saturating_add(height.saturating_sub(margin));
        let max = self
            .render
            .transcript_cache
            .layout
            .total_rows()
            .saturating_sub(height);
        if geometry.rows.start < top_threshold {
            scroll.offset = geometry.rows.start.saturating_sub(margin).min(max);
            scroll.follow = false;
        } else if geometry.rows.end > bottom_threshold {
            scroll.offset = geometry
                .rows
                .end
                .saturating_add(margin)
                .saturating_sub(height)
                .min(max);
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
    use crate::{
        reading::{
            BlockId, ReadingBlock, ReadingBlockKind, ReadingBlockLayout, ReadingCopyPayload,
        },
        DirtyState, UiAction,
    };
    use ratatui::text::Line;

    #[test]
    fn session_usage_retains_valid_sample_across_empty_updates() {
        let mut session = SessionModel::default();
        session.record_usage(Some(1), Some(0), Some(TokenUsage::default()));
        assert_eq!(session.context_usage_percent(1_000), 0);
        assert_eq!(session.last_usage_sample, None);
        assert_eq!(session.cache_hit_rate(), None);

        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 200,
            cache_write_tokens: 50,
        };
        session.record_usage(Some(1), Some(0), Some(usage));
        for (turn, step) in [(1, 0), (1, 1), (2, 0)] {
            for empty in [None, Some(TokenUsage::default())] {
                session.record_usage(Some(turn), Some(step), empty);
                assert_eq!(session.context_usage_percent(1_000), 40);
                assert_eq!(session.last_usage_sample, Some((Some(1), Some(0), usage)));
                assert_eq!(session.token_usage, usage);
                assert_eq!(session.cache_hit_rate(), Some(57));
            }
        }

        let corrected = TokenUsage {
            output_tokens: 100,
            ..usage
        };
        session.record_usage(Some(1), Some(0), Some(corrected));
        assert_eq!(session.context_usage_percent(1_000), 45);
        assert_eq!(session.token_usage, corrected);

        let smaller = TokenUsage {
            input_tokens: 50,
            output_tokens: 10,
            ..TokenUsage::default()
        };
        session.record_usage(Some(2), Some(0), Some(smaller));
        assert_eq!(session.context_usage_percent(1_000), 6);
        assert_eq!(session.last_usage_sample, Some((Some(2), Some(0), smaller)));
        assert_eq!(session.token_usage.input_tokens, 150);
        assert_eq!(session.token_usage.output_tokens, 110);
        assert_eq!(session.token_usage.cache_read_tokens, 200);
        assert_eq!(session.token_usage.cache_write_tokens, 50);
    }

    #[test]
    fn normal_preview_ignores_plain_and_user_owned_content() {
        let mut app = TuiApp::default();
        app.timeline.transcript.append(
            DisplayItem::Activity(crate::display::ActivityRow::root(
                crate::display::DisplayId::correlated("command", "build"),
                "/build",
            )),
            None,
        );
        app.reconcile_latest_preview();
        app.preview.scroll = 4;
        let target = app.preview.target.clone();
        let state = app.preview.state.clone();

        app.timeline.transcript.append(
            DisplayItem::Block(crate::display::TranscriptBlock {
                id: crate::display::DisplayId::correlated("system", "plain"),
                unit: None,
                content: "plain system output".into(),
                format: TranscriptFormat::Plain,
                tone: crate::display::DisplayTone::Error,
                copy_source: "plain system output".into(),
                streaming: false,
            }),
            None,
        );
        for (role, id) in [
            (CardRole::User, "user"),
            (CardRole::Attachment, "attachment"),
        ] {
            app.timeline.transcript.append(
                DisplayItem::Card(crate::display::ContentCard {
                    id: crate::display::DisplayId::correlated("message", id),
                    unit: None,
                    header: None,
                    content: id.into(),
                    role,
                    tone: crate::display::DisplayTone::Normal,
                    horizontal_padding: 1,
                    copy_source: id.into(),
                }),
                None,
            );
        }
        app.reconcile_latest_preview();

        assert_eq!(app.preview.target, target);
        assert_eq!(app.preview.state, state);
        assert_eq!(app.preview.scroll, 4);
    }

    #[test]
    fn ignored_content_is_empty_normally_but_remains_available_to_reading() {
        let mut app = TuiApp::default();
        app.timeline.transcript.append(
            DisplayItem::Card(crate::display::ContentCard {
                id: crate::display::DisplayId::correlated("message", "user"),
                unit: Some(1),
                header: None,
                content: "user prompt".into(),
                role: CardRole::User,
                tone: crate::display::DisplayTone::Normal,
                horizontal_padding: 1,
                copy_source: "user prompt".into(),
            }),
            None,
        );
        app.reconcile_latest_preview();
        assert!(app.preview.target.is_none());
        assert_eq!(app.preview.state, crate::PreviewState::Empty);

        let document = ReadingDocument::derive(&app.timeline, &app.render, &app.config);
        assert!(matches!(
            document.blocks.as_slice(),
            [ReadingBlock {
                preview: PreviewRef::Inline {
                    content: PreviewContent::PlainText(text),
                    ..
                },
                ..
            }] if text == "user prompt"
        ));
    }

    #[test]
    fn activity_spinner_uses_the_requested_braille_sequence() {
        let mut app = TuiApp::default();
        for (index, expected) in ACTIVITY_SPINNER_FRAMES.iter().enumerate() {
            app.render.activity_frame = index;
            assert_eq!(app.activity_spinner_frame(), *expected);
        }
        app.render.activity_frame = ACTIVITY_SPINNER_FRAMES.len();
        assert_eq!(app.activity_spinner_frame(), ACTIVITY_SPINNER_FRAMES[0]);
    }

    #[test]
    fn reading_page_margin_uses_a_stable_ceiling_quarter() {
        let id = BlockId("selected".into());
        let preview = PreviewRef::Inline {
            key: PreviewKey("selected".into()),
            revision: PreviewRevision(1),
            content: PreviewContent::PlainText("selected".into()),
        };
        let document = ReadingDocument {
            blocks: vec![ReadingBlock {
                id: id.clone(),
                owner: crate::display::DisplayId::correlated("reading", "selected"),
                unit: None,
                kind: ReadingBlockKind::Notice,
                copy: ReadingCopyPayload {
                    text: "selected".into(),
                    atomic: false,
                },
                preview,
                items: Vec::new(),
            }],
        };
        let mut app = TuiApp::default();
        app.render.transcript_cache.lines = vec![Line::from("row"); 100];
        app.render
            .transcript_cache
            .ensure_layout(80, crate::transcript_layout::wrapped_rows);
        app.reading_document = document;
        app.reading_layout = ReadingLayout {
            width: 80,
            blocks: vec![ReadingBlockLayout {
                block_id: id.clone(),
                rows: 24..25,
                gutter_x: 0,
                items: Vec::new(),
            }],
        };
        app.reading = ReadingViewState::enter(
            &app.reading_document,
            &app.reading_layout,
            0..30,
            &InputState::new(&app.config),
        );

        let mut scroll = ScrollState::default();
        app.keep_reading_visible(&mut scroll, 30);
        assert_eq!(scroll.offset, 3);
        app.keep_reading_visible(&mut scroll, 30);
        assert_eq!(scroll.offset, 3, "downward page placement is stable");

        app.reading_layout.blocks[0].rows = 10..11;
        scroll.offset = 10;
        app.keep_reading_visible(&mut scroll, 30);
        assert_eq!(scroll.offset, 2);
        app.keep_reading_visible(&mut scroll, 30);
        assert_eq!(scroll.offset, 2, "upward page placement is stable");
    }

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
