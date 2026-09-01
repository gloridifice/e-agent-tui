//! Provider-neutral runtime state and normalized timeline reduction.
//!
//! Agent adapters provide typed timeline records. This module folds those
//! records into the frontend's display model without retaining provider wire
//! names or transport values.

#[cfg(test)]
use ratatui::style::Color;

use crate::AgentRequest;

const FRONTEND_REPLAY_EVENT_CAP: usize = 2_000;
pub use crate::app::{
    breathing_color, lerp_color, settle_color, BREATH_CYCLE_MS, SETTLE_TRANSITION_MS,
};
use crate::display::{
    ActivityRow, ActivityState, CardRole, DisplayId, DisplayItem, DisplayTone, ThinkingNode,
    TranscriptBlock, TranscriptFormat,
};
use crate::projection::{
    assistant::{self, AssistantMutation},
    command::{self, CommandProjection},
    is_surface_node,
    lifecycle::{self, LifecycleProjection},
    retry,
    tool::ToolMutation,
    workflow::{self, WorkflowProjection},
    AccessoryStateEffect, ActivityMutation, EventProjector, PageStateEffect,
    PendingActivityEnrichment, PendingActivityResult, PendingToolResult, ProjectionEffect,
};
#[cfg(test)]
use crate::render::RenderLine;
use crate::render::RenderOptions;
use crate::{
    agent::timeline::{SurfaceOperation, TimelineFact, TimelineRecord, TokenUsage},
    preview::{
        MutationDiff, MutationHunk, ToolMetrics, ToolPreview, ToolPreviewPrimary,
        ToolPreviewSecondary,
    },
    ActivityTransition, PreviewContent, PreviewKey, PreviewRef, PreviewRevision, TuiApp,
};

/// Test-only characterization model retained while fixtures are rewritten.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ToolCard {
    pub call_id: String,
    pub name: String,
    /// Short human summary: the command text for shell tools, otherwise
    /// trimmed arguments.
    pub summary: String,
    pub state: ToolState,
    /// Frame index into the spinner frames while running.
    pub frame: usize,
    /// Event time of the normalized tool start (duration base).
    pub start_ms: u64,
    /// Settle transition: captured at completion, animated toward the done
    /// color instead of snapping (None = not yet settled / replayed).
    pub done_since: Option<std::time::Instant>,
    pub done_from: Option<Color>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum ToolState {
    Running,
    /// Done: ok = exit status zero (or no exit marker), lines = output size.
    Done {
        ok: bool,
        lines: usize,
        lines_truncated: bool,
        duration_ms: u64,
    },
}

/// File operations that can share one folded activity group. The operation
/// name remains visible (`read`, `view`, `edit`, `replace`, `insert`) even
/// when several kinds settle onto the same line.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileAction {
    Read,
    View,
    Edit,
    Replace,
    Insert,
    Create,
}

#[cfg(test)]
impl FileAction {
    pub const FOLD_ORDER: [Self; 5] = [
        Self::Read,
        Self::View,
        Self::Edit,
        Self::Replace,
        Self::Insert,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::View => "view",
            Self::Edit => "edit",
            Self::Replace => "replace",
            Self::Insert => "insert",
            Self::Create => "create",
        }
    }

    pub fn is_read_like(self) -> bool {
        matches!(self, Self::Read | Self::View)
    }

    pub fn foldable(self) -> bool {
        self != Self::Create
    }
}

/// A merged group of consecutive file operations rendered on one line, for
/// example `read a.rs; view b.rs; replace c.rs`. Creates deliberately remain
/// standalone because they introduce a new file rather than mutate/read one.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FileGroup {
    pub items: Vec<FileItem>,
    pub frame: usize,
    /// Settle transition (whole group): captured when the last pending item
    /// settles, animated toward umber/red instead of snapping.
    pub done_since: Option<std::time::Instant>,
    pub done_from: Option<Color>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FileItem {
    pub action: FileAction,
    pub call_id: String,
    pub file: String,
    /// None = still pending.
    pub ok: Option<bool>,
}

#[cfg(test)]
impl FileGroup {
    pub fn pending(&self) -> bool {
        self.items.iter().any(|item| item.ok.is_none())
    }
}

/// Screen rows used by the legacy characterization fixture. Failed items render one
/// line per DISTINCT action/file pair (repeats collapse into `name xN`).
#[cfg(test)]
pub fn file_group_line_count(group: &FileGroup) -> usize {
    let failed = |read_like: bool| -> usize {
        FileAction::FOLD_ORDER
            .iter()
            .copied()
            .filter(|action| action.is_read_like() == read_like)
            .map(|action| {
                group
                    .items
                    .iter()
                    .filter(|item| item.action == action && item.ok == Some(false))
                    .map(|item| item.file.as_str())
                    .collect::<std::collections::HashSet<_>>()
                    .len()
            })
            .sum()
    };
    let any = |read_like: bool, ok: Option<bool>| {
        group
            .items
            .iter()
            .any(|item| item.action.is_read_like() == read_like && item.ok == ok)
    };
    if any(true, None) {
        // breathing read/view line + settled read/view failures
        1 + failed(true)
    } else if any(false, None) {
        // folded successful reads/views + their failures + breathing write
        // line + settled write failures
        usize::from(any(true, Some(true))) + failed(true) + 1 + failed(false)
    } else {
        // one folded success line + one line per distinct failed action/file.
        usize::from(group.items.iter().any(|item| item.ok == Some(true)))
            + failed(true)
            + failed(false)
    }
}

/// One renderable message row/card in the transcript.
#[cfg(test)]
#[derive(Debug, Clone)]
pub enum LegacyTestMsg {
    /// Shared ordinary transcript display surface.
    Block(crate::display::TranscriptBlock),
    /// Shared padded content-card display surface.
    Card(crate::display::ContentCard),
    /// Shared status-bearing activity display surface.
    Activity(crate::display::ActivityRow),
    /// User message, shown verbatim (D20).
    User {
        text: String,
    },
    /// Assistant text with its pre-rendered markdown lines (D8).
    Assistant {
        /// Original markdown source (copy mode copies this).
        text: String,
        lines: Vec<RenderLine>,
        /// First unit id owned by this message; re-renders reuse the range so
        /// unit references (copy mode, expanded set) stay valid.
        unit_start: u64,
    },
    /// Streaming assistant text (replaced by Assistant when assembled).
    Streaming {
        text: String,
    },
    Tool(ToolCard),
    /// Model-thinking phase (between user send / tool results and the next
    /// visible activity), rendered like a tool card: breathing bullet while
    /// the model thinks, green once the phase completes.
    Thinking(ThinkingCard),
    /// Merged consecutive read/edit calls on one line.
    FileGroup(FileGroup),
    /// System / lifecycle notices (session start, compaction …).
    System {
        text: String,
    },
    Error {
        text: String,
    },
}

#[cfg(test)]
pub type Msg = LegacyTestMsg;

/// Lifecycle of the "Thinking..." row.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThinkState {
    /// The model is between visible events (or waiting for its turn).
    Running,
    /// Visible activity took over or the turn ended.
    Done,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ThinkingCard {
    pub state: ThinkState,
    /// Number of consecutive thinking phases this row represents
    /// (`Thinking... xN` when > 1).
    pub count: usize,
    /// Settle transition: captured at completion, animated from the
    /// breathing color to green instead of snapping.
    pub done_since: Option<std::time::Instant>,
    pub done_from: Option<Color>,
    /// Accumulated reasoning content (merged Thinking+Reasoning node).
    pub content: String,
    /// Copy unit of the accumulated reasoning content.
    pub unit: Option<u64>,
}

#[cfg(test)]
impl ThinkingCard {
    /// Test mirror of the merged `ThinkingNode` display surface.
    pub fn from_node(node: &crate::display::ThinkingNode) -> Self {
        Self {
            state: if node.row.state == ActivityState::Running {
                ThinkState::Running
            } else {
                ThinkState::Done
            },
            count: node.row.count,
            done_since: None,
            done_from: None,
            content: node.content.clone(),
            unit: node.unit,
        }
    }
}

pub use crate::{
    interaction::ApprovalCard, question::QuestionBatch, NewConversationDraft,
    SessionStatus as AgentStatus,
};

pub struct RuntimeState {
    /// Kernel-neutral application root. Existing field-style callers are
    /// temporarily forwarded through `Deref` while lifecycle owners move.
    pub tui: TuiApp,
    #[cfg(test)]
    pub msgs: Vec<Msg>,
}

impl std::ops::Deref for RuntimeState {
    type Target = TuiApp;

    fn deref(&self) -> &Self::Target {
        &self.tui
    }
}

impl std::ops::DerefMut for RuntimeState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tui
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            tui: TuiApp::default(),
            #[cfg(test)]
            msgs: Vec::new(),
        }
    }
}

impl RuntimeState {
    pub fn theme(&self) -> crate::config::Theme {
        self.config.theme()
    }

    pub fn cache_hit_rate(&self) -> Option<u64> {
        self.session.cache_hit_rate()
    }

    fn record_usage(&mut self, turn: Option<u64>, step: Option<u64>, usage: Option<TokenUsage>) {
        self.session.record_usage(turn, step, usage);
    }

    /// Breathing phase in [0, 1) from the current activity epoch; 0 while no
    /// activity has started (gray).
    pub fn breath_phase(&self) -> f64 {
        match self.session.activity_epoch {
            Some(epoch) => {
                let elapsed = epoch.elapsed().as_millis() % BREATH_CYCLE_MS;
                elapsed as f64 / BREATH_CYCLE_MS as f64
            }
            None => 0.0,
        }
    }

    /// The model started thinking (or the user sent a message before the
    /// turn exists): show a breathing `• Thinking...` row. Only ADJACENT
    /// thinking phases collapse into one row with a count (`xN`) — any
    /// visible activity in between starts a fresh row. History replays
    /// update the flag without pushing rows.
    pub fn start_thinking(&mut self) {
        self.session.working = true;
        if self.replaying {
            return;
        }
        let adjacent_id = self.transcript.nodes().last().and_then(|node| {
            matches!(&node.item, DisplayItem::Thinking(node)
                if node.row.id.0.starts_with("thinking:"))
            .then(|| node.id().clone())
        });
        if let Some(id) = adjacent_id {
            if let Some(node) = self.transcript.get_mut(&id) {
                if let DisplayItem::Thinking(thinking) = &mut node.item {
                    if thinking.row.state != ActivityState::Running {
                        thinking.row.state = ActivityState::Running;
                        thinking.row.count += 1;
                    }
                }
            }
            self.transcript.touch(&id);
        } else {
            let id = DisplayId::correlated("thinking", &self.next_thinking_id.to_string());
            self.next_thinking_id = self.next_thinking_id.wrapping_add(1);
            let mut row = ActivityRow::root(id, "Thinking...");
            row.count = 1;
            self.insert_transcript_item(
                DisplayItem::Thinking(ThinkingNode {
                    row,
                    unit: None,
                    content: String::new(),
                    copy_source: String::new(),
                    streaming: false,
                    turn: None,
                }),
                None,
                None,
            );
        }

        #[cfg(test)]
        match self.msgs.last_mut() {
            Some(Msg::Thinking(card)) if card.state == ThinkState::Running => {}
            Some(Msg::Thinking(card)) => {
                card.state = ThinkState::Running;
                card.done_since = None;
                card.done_from = None;
                card.count += 1;
                self.render.transcript_cache.valid = false;
            }
            _ => {
                self.msgs.push(Msg::Thinking(ThinkingCard {
                    state: ThinkState::Running,
                    count: 1,
                    done_since: None,
                    done_from: None,
                    content: String::new(),
                    unit: None,
                }));
                self.render.transcript_cache.valid = false;
            }
        }
    }

    fn capture_activity_transition(&mut self, id: &DisplayId) {
        if self.replaying || self.render.activity_transitions.contains_key(id) {
            return;
        }
        let from = breathing_color(&self.config.theme(), self.breath_phase());
        self.render.activity_transitions.insert(
            id.clone(),
            ActivityTransition {
                done_since: std::time::Instant::now(),
                from,
            },
        );
    }

    /// The thinking phase ended (visible activity took over, the turn
    /// ended, or the agent went idle): settle the running Thinking row green.
    pub fn stop_thinking(&mut self) {
        self.session.working = false;
        if self.replaying {
            return;
        }
        let running_id = self.transcript.nodes().iter().rev().find_map(|node| {
            matches!(&node.item, DisplayItem::Thinking(node)
                if node.row.id.0.starts_with("thinking:")
                    && node.row.state == ActivityState::Running)
            .then(|| node.id().clone())
        });
        if let Some(id) = running_id {
            self.capture_activity_transition(&id);
            if let Some(node) = self.transcript.get_mut(&id) {
                if let DisplayItem::Thinking(thinking) = &mut node.item {
                    thinking.row.state = ActivityState::Success;
                }
            }
            self.transcript.touch(&id);
        }
        #[cfg(test)]
        {
            let breath_now = breathing_color(&self.config.theme(), self.breath_phase());
            if let Some(Msg::Thinking(card)) =
                self.msgs.iter_mut().rev().find(
                    |msg| matches!(msg, Msg::Thinking(card) if card.state == ThinkState::Running),
                )
            {
                card.state = ThinkState::Done;
                card.done_since = Some(std::time::Instant::now());
                card.done_from = Some(breath_now);
                self.render.transcript_cache.valid = false;
            }
        }
    }

    pub fn push_system_message(&mut self, text: impl Into<String>) {
        self.push_local_block(text.into(), DisplayTone::Info, "system");
    }

    /// Append complete frontend-only Markdown without creating an agent event.
    pub fn push_local_markdown(&mut self, text: impl Into<String>) {
        let text = text.into();
        let id = DisplayId::correlated("help", &format!("local:{}", self.next_local_display_id));
        self.next_local_display_id = self.next_local_display_id.wrapping_add(1);
        let block = TranscriptBlock {
            id,
            unit: None,
            content: text.clone(),
            format: TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: text,
            streaming: false,
        };
        self.insert_transcript_item(DisplayItem::Block(block.clone()), None, None);
        #[cfg(test)]
        self.msgs.push(Msg::Block(block));
        self.render.transcript_cache.invalidate();
    }

    pub fn push_error_message(&mut self, text: impl Into<String>) {
        self.push_local_block(text.into(), DisplayTone::Error, "error");
    }

    fn push_local_block(&mut self, text: String, tone: DisplayTone, role: &str) {
        let id = DisplayId::correlated(role, &format!("local:{}", self.next_local_display_id));
        self.next_local_display_id = self.next_local_display_id.wrapping_add(1);
        let unit = self.allocate_copy_unit(&text);
        self.insert_transcript_item(
            DisplayItem::Block(TranscriptBlock {
                id,
                unit: Some(unit),
                content: text.clone(),
                format: TranscriptFormat::Plain,
                tone,
                copy_source: text.clone(),
                streaming: false,
            }),
            None,
            None,
        );
        #[cfg(test)]
        if tone == DisplayTone::Error {
            self.msgs.push(Msg::Error { text });
        } else {
            self.msgs.push(Msg::System { text });
        }
        self.render.transcript_cache.invalidate();
    }

    pub fn begin_command_execution(&mut self) {
        self.session.active_commands = self.session.active_commands.saturating_add(1);
    }

    pub fn finish_command_execution(&mut self) {
        self.session.active_commands = self.session.active_commands.saturating_sub(1);
    }

    pub fn has_active_command(&self) -> bool {
        self.session.active_commands > 0
    }

    /// Apply the direct command acknowledgment without duplicating the
    /// durable command/run↔command/done activity already seen on the log.
    pub fn apply_command_result(&mut self, command_id: &str, kind: &str, text: Option<&str>) {
        if let Some(id) = self.projector.commands.get(command_id).cloned() {
            self.settle_activity(&id, kind != "error", text);
            return;
        }
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            if kind == "error" {
                self.push_error_message(text);
            } else {
                self.push_system_message(text);
            }
        }
    }

    /// Queue a prompt typed while the agent runs. Returns true when the
    /// caller must send it immediately instead (the agent is idle). The queue
    /// is passed in because the caller may hold the InteractionModel outside
    /// the RuntimeState lock (main-loop take/restore); the queue must never be a
    /// transient default.
    pub fn enqueue_or_immediate(
        &mut self,
        prompt: &crate::PromptInput,
        queue: &mut Vec<crate::PromptInput>,
    ) -> bool {
        if self.session.status == AgentStatus::Running {
            queue.push(prompt.clone());
            false
        } else {
            true
        }
    }

    /// The agent went idle: pop the next queued prompt for auto-dispatch,
    /// one at a time (each dispatch keeps the agent busy until it returns
    /// to idle again).
    pub fn take_next_queued(&mut self) -> Option<crate::PromptInput> {
        if self.session.status == AgentStatus::Idle
            && !self.session.working
            && !self.interaction.queue.is_empty()
        {
            Some(self.interaction.queue.remove(0))
        } else {
            None
        }
    }

    /// Settle everything still running when a turn ends: interrupted turns
    /// leave tool cards, file-group items, and Thinking rows without their
    /// result events — without this they would keep breathing forever (the
    /// Esc-interrupt bug). A dangling streaming tail is finalized into a
    /// regular assistant message so its breathing bullet also stops.
    pub fn settle_turn(&mut self, now_ms: u64, cancelled: bool) {
        let active_ids = self
            .transcript
            .nodes()
            .iter()
            .filter_map(|node| match &node.item {
                DisplayItem::Activity(row) if row.state.is_active() => Some(row.id.clone()),
                DisplayItem::Block(block) if block.streaming => Some(block.id.clone()),
                DisplayItem::Thinking(node) if node.row.state.is_active() || node.streaming => {
                    Some(node.row.id.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for id in active_ids {
            if self
                .transcript
                .get(&id)
                .is_some_and(|node| matches!(&node.item, DisplayItem::Activity(_)))
            {
                self.capture_activity_transition(&id);
            }
            if let Some(node) = self.transcript.get_mut(&id) {
                match &mut node.item {
                    DisplayItem::Activity(row) if row.id.0.starts_with("thinking:") => {
                        row.state = ActivityState::Success;
                    }
                    DisplayItem::Activity(row) => {
                        row.state = if cancelled {
                            ActivityState::Cancelled
                        } else {
                            ActivityState::Failure
                        };
                        row.duration_ms = row.start_ms.map(|start| now_ms.saturating_sub(start));
                    }
                    DisplayItem::Thinking(thinking) => {
                        thinking.row.state = ActivityState::Success;
                        thinking.streaming = false;
                    }
                    DisplayItem::Block(block) => block.streaming = false,
                    _ => {}
                }
            }
            self.transcript.touch(&id);
        }
        #[cfg(test)]
        {
            let breath_now = breathing_color(&self.config.theme(), self.breath_phase());
            for msg in self.msgs.iter_mut() {
                match msg {
                    Msg::Tool(card) => {
                        if card.state == ToolState::Running {
                            let duration_ms = now_ms.saturating_sub(card.start_ms);
                            card.state = ToolState::Done {
                                ok: false,
                                lines: 0,
                                lines_truncated: false,
                                duration_ms,
                            };
                            card.done_since = Some(std::time::Instant::now());
                            card.done_from = Some(breath_now);
                        }
                    }
                    Msg::FileGroup(group) => {
                        for item in &mut group.items {
                            if item.ok.is_none() {
                                item.ok = Some(false);
                            }
                        }
                        settle_group(group, breath_now);
                    }
                    Msg::Thinking(card) => {
                        if card.state == ThinkState::Running {
                            card.state = ThinkState::Done;
                            card.done_since = Some(std::time::Instant::now());
                            card.done_from = Some(breath_now);
                        }
                    }
                    Msg::Activity(row) if row.state.is_active() => {
                        row.state = if cancelled {
                            ActivityState::Cancelled
                        } else {
                            ActivityState::Failure
                        };
                        row.duration_ms = row.start_ms.map(|start| now_ms.saturating_sub(start));
                    }
                    Msg::Block(block) if block.streaming => block.streaming = false,
                    _ => {}
                }
            }
            if let Some(Msg::Streaming { text }) = self.msgs.last() {
                let text = text.clone();
                self.msgs.pop();
                if !text.is_empty() {
                    self.msgs.push(Msg::Assistant {
                        text,
                        lines: Vec::new(),
                        unit_start: self.render.next_unit,
                    });
                }
            }
        }
        self.session.working = false;
        self.render.transcript_cache.valid = false;
    }
}

impl RuntimeState {
    /// Drop the whole transcript (used when attaching to another session).
    pub fn reset_transcript(&mut self) {
        self.transcript.clear();
        self.render.markdown_layout.clear();
        self.render.activity_transitions.clear();
        self.render.transcript_reveals.clear();
        self.preview.reveal = None;
        #[cfg(test)]
        self.msgs.clear();
        self.projector = EventProjector::default();
        self.todos.clear();
        self.goal = None;
        self.plan_mode = None;
        self.session.current_mode = None;
        self.session.current_mode_seq = None;
        self.session.token_usage = TokenUsage::default();
        self.session.last_usage_sample = None;
        self.session_state_events.clear();
        self.render.units.clear();
        self.render.expanded.clear();
        self.render.next_unit = 0;
        self.next_thinking_id = 0;
        self.next_local_display_id = 0;
        self.pending_transcript_insert = None;
        self.replay_newer_display_ids.clear();
        self.session.snapshot_truncated = false;
        self.render.transcript_cache.reset();
        self.session.min_seq = None;
        self.session.history_loading = false;
        self.session.history_exhausted = false;
        self.session.working = false;
        self.session.active_commands = 0;
        self.session.activity_epoch = None;
        // The title belongs to the session being left (welcome sets the
        // new one right after the switch).
        self.session.session_title = None;
        self.session.session_cwd = None;
        // Queued prompts belong to the session they were typed for.
        self.interaction.queue.clear();
    }
}

impl RuntimeState {
    pub fn begin_new_conversation(&mut self, mode: impl Into<String>) {
        self.session.new_conversation = Some(NewConversationDraft {
            mode: mode.into(),
            pending_input: None,
            notice: None,
        });
        // The `/new` page starts empty: the previous session's preview must
        // not carry over into the draft page.
        self.preview.clear();
    }

    /// Retain the first prompt and return the atomic materialization payload.
    /// A second submission while creation is in flight is rejected by the
    /// controller rather than entering the retained old session's queue.
    pub fn materialize_new_conversation(
        &mut self,
        prompt: crate::PromptInput,
    ) -> Option<AgentRequest> {
        let draft = self.session.new_conversation.as_mut()?;
        if draft.pending_input.is_some() {
            return None;
        }
        draft.pending_input = Some(prompt.clone());
        draft.notice = Some("正在创建新对话…".into());
        Some(AgentRequest::NewInput {
            mode: draft.mode.clone(),
            prompt,
        })
    }

    pub fn restore_new_conversation_input(&mut self) -> Option<crate::PromptInput> {
        let draft = self.session.new_conversation.as_mut()?;
        draft.notice = None;
        draft.pending_input.take()
    }

    pub fn set_new_conversation_notice(&mut self, notice: impl Into<String>) {
        if let Some(draft) = self.session.new_conversation.as_mut() {
            draft.notice = Some(notice.into());
        }
    }

    pub fn is_new_conversation(&self) -> bool {
        self.session.new_conversation.is_some()
    }

    /// Apply a typed replay window. The client replay budget is generated
    /// from the canonical wire contract and protects the local render cache.
    pub fn apply_snapshot(&mut self, events: &[TimelineRecord], bridge_truncated: bool) {
        let mut surface: Vec<&TimelineRecord> = events
            .iter()
            .filter(|event| event.is_replay_relevant())
            .collect();
        let truncated = bridge_truncated || surface.len() > FRONTEND_REPLAY_EVENT_CAP;
        if truncated {
            surface = surface.split_off(surface.len().saturating_sub(FRONTEND_REPLAY_EVENT_CAP));
            self.session.snapshot_truncated = true;
            self.push_system_message("（历史较长，仅回放最近消息）");
        }
        self.replaying = true;
        for event in surface {
            self.apply_host_event(event);
        }
        self.replaying = false;
        if self.session.working {
            self.start_thinking();
        }
        self.session.history_exhausted = !truncated;
        self.session.history_loading = false;
    }

    /// Classify one typed host event, apply semantic surface effects, then
    /// reduce the event into the compatibility message projection.
    pub fn apply_host_event(&mut self, event: &TimelineRecord) {
        if let Some(seq) = event.sequence {
            self.session.min_seq = Some(self.session.min_seq.map_or(seq, |m| m.min(seq)));
        }
        let effects = self.projector.effects(event);
        #[cfg(test)]
        let mut replacement_insert = None;
        for effect in effects {
            match effect {
                ProjectionEffect::SurfaceMutation {
                    mut remove_indices,
                    insert_at,
                } => {
                    if let Some(SurfaceOperation::Replace { start, end }) = event.surface {
                        self.pending_transcript_insert = self.transcript.first_surface_position(
                            &event.source_sequences,
                            start,
                            end,
                        );
                        self.transcript
                            .remove_surfaces(&event.source_sequences, start, end);
                    }
                    remove_indices.sort_unstable_by(|left, right| right.cmp(left));
                    for index in remove_indices {
                        #[cfg(test)]
                        if index < self.msgs.len() {
                            self.msgs.remove(index);
                        }
                        self.projector.remove_display_index(index);
                    }
                    #[cfg(test)]
                    {
                        replacement_insert = insert_at;
                    }
                    #[cfg(not(test))]
                    let _ = insert_at;
                    self.render.transcript_cache.invalidate();
                }
                ProjectionEffect::Reduce { insert_at } => {
                    #[cfg(not(test))]
                    let _ = insert_at;
                    #[cfg(test)]
                    let before = self.msgs.len();
                    if let Some(mutations) =
                        assistant::project(event, self.config.user_input_padding)
                    {
                        self.reduce_assistant_event(event, mutations);
                    } else if let Some(mutation) = self.project_tool_family(event) {
                        self.reduce_tool_family(event, mutation);
                    } else {
                        let handled = self.reduce_activity_families(event);
                        debug_assert!(
                            handled,
                            "unhandled typed TimelineRecord family: {:?}",
                            event.fact
                        );
                    }
                    #[cfg(test)]
                    let target = insert_at.or(replacement_insert);
                    #[cfg(test)]
                    if let Some(target) = target.filter(|target| *target < before) {
                        let appended: Vec<Msg> = self.msgs.drain(before..).collect();
                        self.msgs.splice(target..target, appended);
                    }
                    self.record_surface_owner(event);
                }
                ProjectionEffect::Display(item) => {
                    match item {
                        crate::display::DisplayItem::Block(mut block) => {
                            if block.unit.is_none() {
                                block.unit = Some(self.allocate_copy_unit(&block.copy_source));
                            }
                            let surface_seq = event
                                .sequence
                                .filter(|_| event.surface == Some(SurfaceOperation::Append));
                            self.insert_transcript_item(
                                DisplayItem::Block(block.clone()),
                                surface_seq,
                                None,
                            );
                            let index = self.transcript.len().saturating_sub(1);
                            #[cfg(test)]
                            self.msgs.push(Msg::Block(block));
                            if let Some(seq) = surface_seq {
                                self.projector.record_surface_owner(seq, index, false);
                            }
                        }
                        crate::display::DisplayItem::Card(mut card) => {
                            if card.unit.is_none() {
                                card.unit = Some(self.allocate_copy_unit(&card.copy_source));
                            }
                            self.insert_transcript_item(
                                DisplayItem::Card(card.clone()),
                                event.sequence.filter(|_| is_surface_node(&event.fact)),
                                None,
                            );
                            #[cfg(test)]
                            self.msgs.push(Msg::Card(card));
                        }
                        crate::display::DisplayItem::Activity(row) => self.upsert_activity(row),
                        crate::display::DisplayItem::Thinking(mut node) => {
                            if node.unit.is_none() && !node.copy_source.is_empty() {
                                node.unit = Some(self.allocate_copy_unit(&node.copy_source));
                            }
                            self.insert_transcript_item(
                                DisplayItem::Thinking(node.clone()),
                                event.sequence.filter(|_| is_surface_node(&event.fact)),
                                None,
                            );
                            #[cfg(test)]
                            self.msgs
                                .push(Msg::Thinking(ThinkingCard::from_node(&node)));
                        }
                        crate::display::DisplayItem::Composite {
                            activity,
                            mut detail,
                        } => {
                            if detail.unit.is_none() {
                                detail.unit = Some(self.allocate_copy_unit(&detail.copy_source));
                            }
                            self.insert_transcript_item(
                                DisplayItem::Composite {
                                    activity: activity.clone(),
                                    detail: detail.clone(),
                                },
                                event.sequence.filter(|_| is_surface_node(&event.fact)),
                                None,
                            );
                            #[cfg(test)]
                            {
                                self.msgs.push(Msg::Activity(activity));
                                self.msgs.push(Msg::Card(detail));
                            }
                        }
                    }
                    self.render.transcript_cache.invalidate();
                }
                ProjectionEffect::PageState(PageStateEffect::Title(title)) => {
                    self.session.session_title = title;
                }
                ProjectionEffect::AccessoryState(AccessoryStateEffect::Todo(todos)) => {
                    self.todos = todos;
                }
                ProjectionEffect::AccessoryState(AccessoryStateEffect::ClearTodo) => {
                    self.todos.clear();
                }
                ProjectionEffect::CompatibilityError(message) => {
                    self.push_error_message(message);
                }
                ProjectionEffect::Ignore => {}
            }
        }
        self.pending_transcript_insert = None;
        let active_ids = self
            .transcript
            .nodes()
            .iter()
            .map(|node| node.id().clone())
            .collect::<std::collections::HashSet<_>>();
        self.preview_refs.retain(|id, _| active_ids.contains(id));
        self.tool_items.retain(|id, _| active_ids.contains(id));
        self.reconcile_latest_preview();
    }

    fn record_surface_owner(&mut self, event: &TimelineRecord) {
        let Some(seq) = event.sequence.filter(|_| is_surface_node(&event.fact)) else {
            return;
        };
        if let Some(index) = self
            .transcript
            .nodes()
            .iter()
            .position(|node| node.surface_seq == Some(seq))
        {
            self.projector.record_surface_owner(
                seq,
                index,
                matches!(event.surface, Some(SurfaceOperation::Replace { .. })),
            );
        }
    }

    fn allocate_copy_unit(&mut self, source: &str) -> u64 {
        let unit = self.render.next_unit;
        self.render.next_unit += 1;
        self.render.units.insert(unit, source.to_owned());
        unit
    }

    fn insert_transcript_item(
        &mut self,
        item: DisplayItem,
        surface_seq: Option<u64>,
        preferred: Option<usize>,
    ) -> usize {
        if let Some(index) = self.pending_transcript_insert.take().or(preferred) {
            self.transcript.insert(index, item, surface_seq)
        } else {
            self.transcript.append(item, surface_seq)
        }
    }

    fn upsert_activity(&mut self, mut row: crate::display::ActivityRow) {
        let id = row.id.clone();
        if let Some(pending) = self.projector.take_activity_result(&id) {
            row.state = pending.state;
            if let Some(summary) = pending.summary {
                row.summary = summary;
            }
        }
        if let Some(node) = self.transcript.get_mut(&id) {
            node.item = DisplayItem::Activity(row.clone());
            self.transcript.touch(&id);
        } else {
            self.insert_transcript_item(DisplayItem::Activity(row.clone()), None, None);
        }
        #[cfg(test)]
        {
            if let Some(existing) = self.msgs.iter_mut().find_map(|msg| match msg {
                Msg::Activity(existing) if existing.id == id => Some(existing),
                _ => None,
            }) {
                *existing = row.clone();
            } else {
                self.msgs.push(Msg::Activity(row.clone()));
            }
        }
        self.render.transcript_cache.invalidate();
    }

    fn apply_pending_activity_enrichments(&mut self) {
        for (id, enrichment) in self.projector.take_activity_enrichments() {
            #[cfg(test)]
            if let Some(row) = self.msgs.iter_mut().find_map(|msg| match msg {
                Msg::Activity(row) if row.id == id => Some(row),
                _ => None,
            }) {
                row.summary = enrichment.summary.clone();
                if enrichment.start_ms.is_some() {
                    row.start_ms = enrichment.start_ms;
                }
                self.render.transcript_cache.invalidate();
            }
            if let Some(node) = self.transcript.get_mut(&id) {
                if let DisplayItem::Activity(row) = &mut node.item {
                    row.summary = enrichment.summary;
                    if enrichment.start_ms.is_some() {
                        row.start_ms = enrichment.start_ms;
                    }
                }
                self.transcript.touch(&id);
            }
        }
    }

    fn settle_activity_state(
        &mut self,
        id: &DisplayId,
        state: ActivityState,
        summary: Option<&str>,
    ) -> bool {
        let was_active = self.transcript.get(id).is_some_and(
            |node| matches!(&node.item, DisplayItem::Activity(row) if row.state.is_active()),
        );
        if was_active && !state.is_active() {
            self.capture_activity_transition(id);
        }
        let mut settled = false;
        #[cfg(test)]
        if let Some(row) = self.msgs.iter_mut().find_map(|msg| match msg {
            Msg::Activity(row) if &row.id == id => Some(row),
            _ => None,
        }) {
            row.state = state;
            if let Some(summary) = summary {
                row.summary = summary.to_owned();
            }
            settled = true;
        }
        if let Some(node) = self.transcript.get_mut(id) {
            if let DisplayItem::Activity(row) = &mut node.item {
                row.state = state;
                if let Some(summary) = summary {
                    row.summary = summary.to_owned();
                }
                settled = true;
            }
            self.transcript.touch(id);
        }
        if settled {
            self.render.transcript_cache.invalidate();
        }
        settled
    }

    fn settle_activity(&mut self, id: &DisplayId, success: bool, summary: Option<&str>) -> bool {
        self.settle_activity_state(
            id,
            if success {
                ActivityState::Success
            } else {
                ActivityState::Failure
            },
            summary,
        )
    }

    fn settle_activity_state_or_remember(
        &mut self,
        id: DisplayId,
        state: ActivityState,
        summary: Option<String>,
    ) {
        if !self.settle_activity_state(&id, state, summary.as_deref()) {
            self.projector
                .remember_activity_result(id, PendingActivityResult { state, summary });
        }
    }

    #[cfg(test)]
    fn apply_tool_result_to_display(
        &mut self,
        call_id: &str,
        output: &str,
        is_error: bool,
        output_truncated: bool,
        now_ms: u64,
    ) -> bool {
        let index = self.msgs.iter().rposition(|msg| match msg {
            Msg::Tool(card) => card.call_id == call_id,
            Msg::FileGroup(group) => group.items.iter().any(|item| item.call_id == call_id),
            _ => false,
        });
        let breath_now = breathing_color(&self.config.theme(), self.breath_phase());
        let Some(msg) = index.and_then(|index| self.msgs.get_mut(index)) else {
            return false;
        };
        let ok = exit_marker(output) == 0 && !is_error;
        match msg {
            Msg::FileGroup(group) => {
                let Some(item) = group.items.iter_mut().find(|item| item.call_id == call_id) else {
                    return false;
                };
                item.ok = Some(ok);
                settle_group(group, breath_now);
            }
            Msg::Tool(card) if card.call_id == call_id && card.state == ToolState::Running => {
                card.state = ToolState::Done {
                    ok,
                    lines: output.lines().count(),
                    lines_truncated: output_truncated,
                    duration_ms: now_ms.saturating_sub(card.start_ms),
                };
                card.done_since = Some(std::time::Instant::now());
                card.done_from = Some(breath_now);
            }
            _ => return false,
        }
        self.render.transcript_cache.invalidate();
        true
    }

    fn settle_retry_activities(&mut self) {
        let ids = self
            .transcript
            .nodes()
            .iter()
            .filter_map(|node| match &node.item {
                DisplayItem::Activity(row)
                    if row.id.0.starts_with("retry:") && row.state.is_active() =>
                {
                    Some(row.id.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.settle_activity_state(&id, ActivityState::Success, None);
        }
    }

    #[cfg(test)]
    fn upsert_reasoning(&mut self, text: &str, streaming: bool) {
        if text.is_empty() {
            return;
        }
        let reasoning_visible = self.config.thinking_display_mode().shows_reasoning();
        let updated = self
            .msgs
            .iter_mut()
            .rev()
            .find(|msg| matches!(msg, Msg::Thinking(_)))
            .and_then(|msg| {
                let Msg::Thinking(card) = msg else {
                    return None;
                };
                if streaming {
                    card.content.push_str(text);
                } else {
                    card.content = text.to_owned();
                }
                Some((card.unit, card.content.clone()))
            });
        if let Some((unit, content)) = updated {
            if let Some(unit) = unit {
                self.render.units.insert(unit, content);
            } else {
                let unit = self.allocate_copy_unit(&content);
                if let Some(Msg::Thinking(card)) = self
                    .msgs
                    .iter_mut()
                    .rev()
                    .find(|msg| matches!(msg, Msg::Thinking(_)))
                {
                    card.unit = Some(unit);
                }
                self.render.units.insert(unit, content);
            }
            if reasoning_visible && !self.replaying {
                self.render.transcript_cache.mark_tail_dirty();
            }
            return;
        }
        let unit = self.allocate_copy_unit(text);
        self.msgs.push(Msg::Thinking(ThinkingCard {
            state: ThinkState::Running,
            count: 1,
            done_since: None,
            done_from: None,
            content: text.to_owned(),
            unit: Some(unit),
        }));
        if reasoning_visible && !self.replaying {
            // Structural append: the previous tail was not a Thinking card,
            // so tail splicing would drop it. Rebuild the cache instead.
            self.render.transcript_cache.invalidate();
        }
    }
    fn reduce_assistant_event(
        &mut self,
        event: &TimelineRecord,
        mutations: Vec<AssistantMutation>,
    ) {
        match &event.fact {
            TimelineFact::UserMessage { .. } => {
                self.projector.tool_family.close_group();
                self.render.transcript_cache.invalidate();
                if self.session.status == AgentStatus::Idle {
                    self.stop_thinking();
                }
            }
            TimelineFact::AssistantChunk {
                text,
                turn,
                step,
                usage,
                ..
            } => {
                self.record_usage(*turn, *step, *usage);
                self.settle_retry_activities();
                if !text.is_empty() {
                    self.projector.tool_family.close_group();
                    self.stop_thinking();
                }
            }
            TimelineFact::AssistantMessage {
                text,
                turn,
                step,
                usage,
                ..
            } => {
                self.record_usage(*turn, *step, *usage);
                if !text.is_empty() {
                    self.projector.tool_family.close_group();
                }
                self.stop_thinking();
                self.settle_retry_activities();
            }
            _ => return,
        }

        for mutation in mutations {
            match mutation {
                AssistantMutation::Append(item) => {
                    self.append_assistant_item(event, item);
                    self.render.transcript_cache.invalidate();
                }
                AssistantMutation::UpsertReasoning(block) => {
                    // Turn identity keeps per-turn reasoning in its own
                    // merged Thinking node; `None` (e.g. replay of events
                    // without a turn) falls back to the trailing node.
                    let turn = match &event.fact {
                        TimelineFact::AssistantChunk { turn, .. }
                        | TimelineFact::AssistantMessage { turn, .. } => *turn,
                        _ => None,
                    };
                    self.upsert_reasoning_store(block.clone(), turn);
                    #[cfg(test)]
                    self.upsert_reasoning(&block.content, block.streaming);
                }
                AssistantMutation::UpsertAnswer(block) => {
                    self.upsert_answer_store(event, block);
                }
            }
        }
    }

    fn append_assistant_item(&mut self, event: &TimelineRecord, mut item: DisplayItem) {
        match &mut item {
            DisplayItem::Block(block) if block.unit.is_none() => {
                block.unit = Some(self.allocate_copy_unit(&block.copy_source));
            }
            DisplayItem::Card(card) if card.unit.is_none() => {
                card.unit = Some(self.allocate_copy_unit(&card.copy_source));
            }
            DisplayItem::Thinking(node) if node.unit.is_none() && !node.copy_source.is_empty() => {
                node.unit = Some(self.allocate_copy_unit(&node.copy_source));
            }
            _ => {}
        }
        let surface_seq = event.sequence.filter(|_| is_surface_node(&event.fact));
        let preferred = matches!(&item, DisplayItem::Card(card) if card.role == CardRole::User)
            .then(|| {
                self.transcript.nodes().last().and_then(|node| {
                    matches!(&node.item, DisplayItem::Thinking(node)
                        if node.row.id.0.starts_with("thinking:")
                            && node.row.state.is_active())
                    .then(|| self.transcript.len().saturating_sub(1))
                })
            })
            .flatten();
        self.insert_transcript_item(item.clone(), surface_seq, preferred);

        // Injected context previews as complete muted Markdown (prompt
        // injection reads as a distinct content kind, never plain text).
        if let DisplayItem::Card(card) = &item {
            if card.role == CardRole::Context {
                let id = card.id.clone();
                let generation = self.transcript.generation();
                self.preview_refs.insert(
                    id.clone(),
                    PreviewRef::Inline {
                        key: PreviewKey(format!("context:{}", id.0)),
                        revision: PreviewRevision(generation),
                        content: PreviewContent::MutedMarkdown(card.copy_source.clone()),
                    },
                );
            }
        }

        #[cfg(test)]
        match item {
            DisplayItem::Card(card) if card.role == CardRole::User => {
                let trailing_thinking = match self.msgs.last() {
                    Some(Msg::Thinking(_)) => self.msgs.pop(),
                    _ => None,
                };
                self.msgs.push(Msg::User { text: card.content });
                if let Some(thinking) = trailing_thinking {
                    self.msgs.push(thinking);
                }
            }
            DisplayItem::Card(card) => self.msgs.push(Msg::Card(card)),
            DisplayItem::Block(block) if block.id.0.ends_with(":context-fallback") => {
                self.msgs.push(Msg::System {
                    text: block.content,
                });
            }
            DisplayItem::Block(block) => self.msgs.push(Msg::Block(block)),
            DisplayItem::Activity(row) => self.msgs.push(Msg::Activity(row)),
            DisplayItem::Thinking(node) => self
                .msgs
                .push(Msg::Thinking(ThinkingCard::from_node(&node))),
            DisplayItem::Composite { activity, detail } => {
                self.msgs.push(Msg::Activity(activity));
                self.msgs.push(Msg::Card(detail));
            }
        }
    }

    fn upsert_reasoning_store(&mut self, incoming: TranscriptBlock, turn: Option<u64>) {
        // Reasoning chunks accumulate into the merged Thinking+Reasoning
        // node. The target node is the trailing one whose turn matches (live
        // nodes created by `start_thinking` carry `turn: None` and adopt the
        // first chunk's turn). When no node exists for this turn — history
        // replay suppresses the breathing indicator — a settled one is
        // created so the content still travels with a node; per-turn keying
        // keeps later turns from merging into an earlier turn's node.
        let tail_id = self.transcript.nodes().iter().rev().find_map(|node| {
            matches!(&node.item, DisplayItem::Thinking(node)
                if node.turn == turn || node.turn.is_none())
            .then(|| node.id().clone())
        });
        let Some(id) = tail_id else {
            let mut row = ActivityRow::root(
                DisplayId::correlated("thinking", &self.next_thinking_id.to_string()),
                "Thinking...",
            );
            self.next_thinking_id = self.next_thinking_id.wrapping_add(1);
            row.count = 1;
            row.state = ActivityState::Success;
            let node = ThinkingNode {
                row,
                unit: Some(self.allocate_copy_unit(&incoming.copy_source)),
                content: incoming.content,
                copy_source: incoming.copy_source,
                streaming: false,
                turn,
            };
            self.insert_transcript_item(DisplayItem::Thinking(node), None, None);
            if self.config.thinking_display_mode().shows_reasoning() && !self.replaying {
                self.render.transcript_cache.invalidate();
            }
            return;
        };
        let existed = self.transcript.get(&id).is_some();
        let mut existing_unit = None;
        let mut updated_source = None;
        {
            let mut node = self.transcript.get_mut(&id);
            if let Some(node) = node.as_deref_mut() {
                if let DisplayItem::Thinking(thinking) = &mut node.item {
                    if incoming.streaming {
                        thinking.content.push_str(&incoming.content);
                        thinking.copy_source.push_str(&incoming.copy_source);
                        thinking.streaming = true;
                    } else {
                        thinking.content = incoming.content;
                        thinking.copy_source = incoming.copy_source;
                        thinking.streaming = false;
                    }
                    if thinking.turn.is_none() {
                        thinking.turn = turn;
                    }
                    existing_unit = thinking.unit;
                    updated_source = Some(thinking.copy_source.clone());
                }
            }
        }
        // A live node created by `start_thinking` has no copy unit yet;
        // allocate one once reasoning exists so the accumulated content is
        // selectable/copyable like any other surface.
        if let Some(source) = &updated_source {
            if existing_unit.is_none() && !source.is_empty() {
                let unit = self.allocate_copy_unit(source);
                existing_unit = Some(unit);
                if let Some(node) = self.transcript.get_mut(&id) {
                    if let DisplayItem::Thinking(thinking) = &mut node.item {
                        thinking.unit = Some(unit);
                    }
                }
            }
        }
        if let (Some(unit), Some(source)) = (existing_unit, updated_source) {
            self.render.units.insert(unit, source);
            self.transcript.touch(&id);
        }
        if self.config.thinking_display_mode().shows_reasoning() && !self.replaying {
            let is_tail = self.transcript.position(&id) == self.transcript.len().checked_sub(1);
            if existed && is_tail {
                self.render.transcript_cache.mark_tail_dirty();
            } else {
                self.render.transcript_cache.invalidate();
            }
        }
    }

    fn upsert_answer_store(&mut self, event: &TimelineRecord, incoming: TranscriptBlock) {
        let id = incoming.id.clone();
        let mut existed = false;
        if let Some(node) = self.transcript.get_mut(&id) {
            if let DisplayItem::Block(block) = &mut node.item {
                existed = true;
                if incoming.streaming {
                    block.content.push_str(&incoming.content);
                    block.copy_source.push_str(&incoming.copy_source);
                } else {
                    block.content = incoming.content.clone();
                    block.copy_source = incoming.copy_source.clone();
                    block.streaming = false;
                }
            }
        }
        if existed {
            self.transcript.touch(&id);
        } else {
            let surface_seq = event.sequence.filter(|_| is_surface_node(&event.fact));
            self.insert_transcript_item(DisplayItem::Block(incoming.clone()), surface_seq, None);
        }
        if !self.replaying {
            self.render
                .transcript_reveals
                .entry(id.clone())
                .or_default();
        }

        if incoming.streaming {
            let is_tail = self.transcript.position(&id) == self.transcript.len().checked_sub(1);
            #[cfg(test)]
            match self.msgs.last_mut() {
                Some(Msg::Streaming { text }) => text.push_str(&incoming.content),
                _ => self.msgs.push(Msg::Streaming {
                    text: incoming.content,
                }),
            }
            if existed {
                if self.render.transcript_reveals.contains_key(&id) {
                    if let Some(index) = self.transcript.position(&id) {
                        self.render.transcript_cache.mark_reveal_dirty(index);
                    }
                } else if is_tail {
                    self.render.transcript_cache.mark_tail_dirty();
                } else {
                    self.render.transcript_cache.invalidate();
                }
            } else {
                self.render.transcript_cache.invalidate();
            }
            return;
        }

        #[cfg(test)]
        if matches!(self.msgs.last(), Some(Msg::Streaming { .. })) {
            self.msgs.pop();
        }
        let source = self
            .transcript
            .get(&id)
            .and_then(|node| match &node.item {
                DisplayItem::Block(block) => Some(block.content.clone()),
                _ => None,
            })
            .unwrap_or_else(|| incoming.content.clone());
        let theme = self.config.theme();
        let options = RenderOptions {
            expanded: self.render.expanded.clone(),
            collapse_rows: self.config.atomic_collapse_rows,
            mermaid_enabled: self.config.mermaid_enabled,
            markdown_strength: Default::default(),
            content_width: Some(self.render.transcript_cache.width),
        };
        let lines = {
            let render = &mut self.render;
            render
                .markdown_layout
                .materialize(
                    &id,
                    &source,
                    &theme,
                    &mut render.next_unit,
                    &options,
                    &mut render.units,
                )
                .to_vec()
        };
        #[cfg(test)]
        let unit_start = self
            .render
            .markdown_layout
            .unit_start(&id)
            .unwrap_or(self.render.next_unit);
        if let Some(primary) = lines.first().map(|line| line.unit) {
            if let Some(node) = self.transcript.get_mut(&id) {
                if let DisplayItem::Block(block) = &mut node.item {
                    block.unit = Some(primary);
                }
            }
        }
        #[cfg(test)]
        self.msgs.push(Msg::Assistant {
            text: source,
            lines,
            unit_start,
        });
        if existed && self.render.transcript_reveals.contains_key(&id) {
            if let Some(index) = self.transcript.position(&id) {
                self.render.transcript_cache.mark_reveal_dirty(index);
            }
        } else {
            self.render.transcript_cache.invalidate();
        }
    }

    fn project_tool_family(&mut self, event: &TimelineRecord) -> Option<ToolMutation> {
        let now_ms = host_event_time(event);
        match &event.fact {
            TimelineFact::ToolCall(_) => {
                let session_cwd = self.session.session_cwd.clone();
                let read_merge = self.config.read_merge;
                self.projector.tool_family.project_call(
                    event,
                    session_cwd.as_deref(),
                    read_merge,
                    now_ms,
                )
            }
            TimelineFact::ToolResult { .. } => {
                self.start_thinking();
                self.projector.tool_family.project_result(event, now_ms)
            }
            TimelineFact::UserMessage { .. }
            | TimelineFact::AssistantChunk { .. }
            | TimelineFact::AssistantMessage { .. } => None,
            _ => {
                self.projector.tool_family.close_group();
                None
            }
        }
    }

    fn reduce_tool_family(&mut self, event: &TimelineRecord, mutation: ToolMutation) {
        let now_ms = host_event_time(event);
        let mut row = match mutation {
            ToolMutation::Upsert(row) => row,
            ToolMutation::Ignore => return,
            ToolMutation::MissingResult => {
                if let TimelineFact::ToolResult {
                    activity_id,
                    output,
                    state,
                    output_truncated,
                    mutation_diff,
                    mutation_hunks,
                } = &event.fact
                {
                    if let Some(seq) = event.sequence {
                        self.projector.record_surface_seq(seq);
                    }
                    self.projector.remember_tool_result(
                        activity_id.clone(),
                        PendingToolResult {
                            output: output.clone(),
                            is_error: !matches!(state, crate::agent::ActivityState::Success),
                            output_truncated: *output_truncated,
                            time_ms: now_ms,
                            surface_seq: event.sequence,
                            mutation_diff: mutation_diff.clone(),
                            mutation_hunks: mutation_hunks.clone(),
                        },
                    );
                }
                return;
            }
        };
        match &event.fact {
            TimelineFact::ToolCall(_) => {
                self.render.transcript_cache.invalidate();
                self.stop_thinking();
            }
            TimelineFact::ToolResult { .. } => self.start_thinking(),
            _ => {}
        }

        let row_id = row.id.clone();
        match &event.fact {
            TimelineFact::ToolCall(activity) => {
                self.projector
                    .tool_calls
                    .insert(activity.id.clone(), row_id.clone());
                // File paths are displayed relative to the workspace
                // (absolute when outside); normalize once here so the stored
                // tool items, Reading items, and previews all agree.
                let workspace = self.session.session_cwd.clone();
                let items = activity
                    .items
                    .iter()
                    .map(|item| crate::agent::tool::ToolItem {
                        reference: item.reference.relativized(workspace.as_deref()),
                        ..item.clone()
                    })
                    .collect::<Vec<_>>();
                self.tool_items.insert(row_id.clone(), items);
                let revision = PreviewRevision(event.sequence.unwrap_or_default());
                let key = PreviewKey(format!("tool:{}", activity.id));
                // Common-format tools carry a structured seed; mutation tools
                // carry their preview through `reference` instead.
                let content = if let Some(preview) = activity.preview.as_ref() {
                    let preview = relativize_tool_preview(preview, workspace.as_deref());
                    self.projector
                        .tool_preview_seeds
                        .insert(activity.id.clone(), preview.clone());
                    Some(PreviewContent::Tool(preview))
                } else {
                    activity.reference.as_ref().and_then(|reference| {
                        reference
                            .relativized(workspace.as_deref())
                            .preview_content()
                    })
                };
                if let Some(content) = content {
                    self.preview_refs.insert(
                        row_id.clone(),
                        PreviewRef::Inline {
                            key,
                            revision,
                            content,
                        },
                    );
                }
            }
            TimelineFact::ToolResult {
                activity_id,
                output,
                output_truncated,
                mutation_diff,
                mutation_hunks,
                ..
            } => {
                let seed = self.projector.tool_preview_seeds.get(activity_id).cloned();
                let metrics = ToolMetrics {
                    output_lines: row.output_lines.unwrap_or(output.lines().count()),
                    truncated: *output_truncated || row.output_lines_truncated,
                    duration_ms: row.duration_ms,
                };
                let content = tool_preview_result(
                    seed.as_ref(),
                    Some((
                        output,
                        *output_truncated,
                        mutation_diff.as_ref(),
                        mutation_hunks,
                    )),
                    Some(metrics),
                    self.session.session_cwd.as_deref(),
                );
                if let Some(content) = content {
                    self.preview_refs.insert(
                        row_id.clone(),
                        PreviewRef::Inline {
                            key: PreviewKey(format!("tool:{activity_id}")),
                            revision: PreviewRevision(event.sequence.unwrap_or_default()),
                            content,
                        },
                    );
                }
            }
            _ => {}
        }
        let pending_result = match &event.fact {
            TimelineFact::ToolCall(activity) => self.projector.take_tool_result(&activity.id),
            _ => None,
        };
        if let Some(pending) = &pending_result {
            row.state = if pending.is_error {
                ActivityState::Failure
            } else {
                ActivityState::Success
            };
            if row.label != "create" {
                row.duration_ms = Some(pending.time_ms.saturating_sub(row.start_ms.unwrap_or(0)));
                row.output_lines = Some(pending.output.lines().count());
                row.output_lines_truncated = pending.output_truncated;
                row.live_duration_since = None;
            }
        }
        // A call that carried a staged result (result-before-call history)
        // finalizes its structured preview from the same settled facts as the
        // live result path.
        if let (TimelineFact::ToolCall(activity), Some(pending)) = (&event.fact, &pending_result) {
            let seed = self.projector.tool_preview_seeds.get(&activity.id).cloned();
            let metrics = ToolMetrics {
                output_lines: row.output_lines.unwrap_or(pending.output.lines().count()),
                truncated: pending.output_truncated || row.output_lines_truncated,
                duration_ms: row.duration_ms,
            };
            let revision = pending
                .surface_seq
                .unwrap_or(event.sequence.unwrap_or_default());
            let content = tool_preview_result(
                seed.as_ref(),
                Some((
                    &pending.output,
                    pending.output_truncated,
                    pending.mutation_diff.as_ref(),
                    &pending.mutation_hunks,
                )),
                Some(metrics),
                self.session.session_cwd.as_deref(),
            );
            if let Some(content) = content {
                self.preview_refs.insert(
                    row_id.clone(),
                    PreviewRef::Inline {
                        key: PreviewKey(format!("tool:{}", activity.id)),
                        revision: PreviewRevision(revision),
                        content,
                    },
                );
            }
        }
        let surface_seq = event.sequence.filter(|_| is_surface_node(&event.fact));
        let was_active = self.transcript.get(&row_id).is_some_and(|node| {
            matches!(&node.item, DisplayItem::Activity(existing) if existing.state.is_active())
        });
        if was_active && !row.state.is_active() {
            self.capture_activity_transition(&row_id);
        }
        if let Some(existing) = self.transcript.get_mut(&row_id) {
            if let DisplayItem::Activity(existing_row) = &mut existing.item {
                let keep_label = existing_row.label.clone();
                let keep_summary = existing_row.summary.clone();
                *existing_row = row.clone();
                if existing_row.label == "tool" {
                    existing_row.label = keep_label;
                    existing_row.summary = keep_summary;
                }
            }
            if let Some(seq) = surface_seq {
                existing.surface_seq = Some(seq);
            }
            self.transcript.touch(&row_id);
        } else {
            self.insert_transcript_item(DisplayItem::Activity(row.clone()), surface_seq, None);
        }
        #[cfg(test)]
        self.upsert_tool_legacy_mirror(event, &row);
        if let Some(pending) = &pending_result {
            if let (Some(seq), Some(index)) =
                (pending.surface_seq, self.transcript.position(&row_id))
            {
                self.projector.record_surface_owner(seq, index, false);
            }
        }

        #[cfg(test)]
        if let (TimelineFact::ToolCall(activity), Some(pending)) = (&event.fact, pending_result) {
            self.apply_tool_result_to_display(
                &activity.id,
                &pending.output,
                pending.is_error,
                pending.output_truncated,
                pending.time_ms,
            );
        }

        #[cfg(test)]
        if let TimelineFact::ToolResult {
            activity_id,
            output,
            state,
            output_truncated,
            ..
        } = &event.fact
        {
            self.apply_tool_result_to_display(
                activity_id,
                output,
                !matches!(state, crate::agent::ActivityState::Success),
                *output_truncated,
                now_ms,
            );
        }
    }

    #[cfg(test)]
    fn upsert_tool_legacy_mirror(&mut self, event: &TimelineRecord, row: &ActivityRow) {
        let TimelineFact::ToolCall(activity) = &event.fact else {
            return;
        };
        let call_id = &activity.id;
        let msg = if row.id.0.starts_with("file-group:") {
            let items = self
                .projector
                .tool_family
                .group_items(&row.id)
                .unwrap_or_default()
                .into_iter()
                .map(|item| FileItem {
                    action: legacy_file_action(item.action.label()),
                    call_id: item.call_id,
                    file: item.file,
                    ok: item.ok,
                })
                .collect();
            Msg::FileGroup(FileGroup {
                items,
                frame: 0,
                done_since: None,
                done_from: None,
            })
        } else {
            Msg::Tool(ToolCard {
                call_id: call_id.clone(),
                name: row.label.clone(),
                summary: row.summary.clone(),
                state: ToolState::Running,
                frame: 0,
                start_ms: row.start_ms.unwrap_or(0),
                done_since: None,
                done_from: None,
            })
        };

        if let Msg::FileGroup(new_group) = &msg {
            let index = self
                .msgs
                .iter()
                .position(|msg| matches!(msg, Msg::FileGroup(group) if group.items.first().is_some_and(|item| row.id == DisplayId::correlated("file-group", &item.call_id))))
                .or_else(|| {
                    self.msgs
                        .iter()
                        .rposition(|msg| !matches!(msg, Msg::Thinking(_)))
                        .filter(|index| matches!(self.msgs[*index], Msg::FileGroup(_)))
                });
            if let Some(index) = index {
                if let Some(Msg::FileGroup(existing)) = self.msgs.get_mut(index) {
                    let old_done = existing.done_since.zip(existing.done_from);
                    *existing = new_group.clone();
                    if let Some((since, from)) = old_done {
                        existing.done_since = Some(since);
                        existing.done_from = Some(from);
                    }
                    self.render.transcript_cache.invalidate();
                    return;
                }
            }
        }
        self.msgs.push(msg);
        self.render.transcript_cache.invalidate();
    }

    fn reduce_activity_families(&mut self, event: &TimelineRecord) -> bool {
        if let Some(projection) = lifecycle::project(event) {
            self.apply_lifecycle_projection(event, projection);
            return true;
        }

        let retry_id = match &event.fact {
            TimelineFact::RetryScheduled { id, .. } | TimelineFact::RetryStarted { id, .. } => {
                Some(id.as_str())
            }
            _ => None,
        };
        let existing_retry_summary = retry_id.and_then(|retry_id| {
            let id = DisplayId::correlated("retry", retry_id);
            self.transcript.get(&id).and_then(|node| match &node.item {
                DisplayItem::Activity(row) => Some(row.summary.clone()),
                _ => None,
            })
        });
        if let Some((key, mutation)) = retry::project(event, existing_retry_summary) {
            let id = mutation_id(&mutation);
            self.projector.retries.insert(key, id);
            self.apply_activity_mutation(mutation);
            return true;
        }

        let (parent_id, parent_depth) = match &event.fact {
            TimelineFact::SubagentStarted {
                root_id, parent_id, ..
            } => {
                let parent_key = if parent_id.is_empty() {
                    root_id
                } else {
                    parent_id
                };
                let id = self
                    .projector
                    .nested_calls
                    .get(parent_key)
                    .or_else(|| self.projector.tool_calls.get(parent_key))
                    .cloned();
                let depth = id
                    .as_ref()
                    .and_then(|id| self.transcript.get(id))
                    .and_then(|node| match &node.item {
                        DisplayItem::Activity(row) => Some(row.depth),
                        _ => None,
                    })
                    .unwrap_or(0);
                (id, depth)
            }
            _ => (None, 0),
        };
        if let Some(projection) = command::project(event, parent_id, parent_depth) {
            match projection {
                CommandProjection::Command { key, mutation } => {
                    self.projector.commands.insert(key, mutation_id(&mutation));
                    self.apply_activity_mutation(mutation);
                }
                CommandProjection::Nested { key, mutation } => {
                    self.projector
                        .nested_calls
                        .insert(key, mutation_id(&mutation));
                    self.apply_activity_mutation(mutation);
                }
            }
            return true;
        }

        if let Some(projection) = workflow::project(event) {
            match projection {
                WorkflowProjection::Workflow { key, mutation } => {
                    if matches!(event.fact, TimelineFact::WorkflowStarted { .. }) {
                        self.projector.workflows.insert(key, mutation_id(&mutation));
                    }
                    self.apply_activity_mutation(mutation);
                }
                WorkflowProjection::Compaction { key, mutation } => {
                    self.projector
                        .compactions
                        .insert(key, mutation_id(&mutation));
                    self.apply_activity_mutation(mutation);
                }
            }
            return true;
        }
        false
    }

    fn apply_activity_mutation(&mut self, mutation: ActivityMutation) {
        match mutation {
            ActivityMutation::Upsert(row) => {
                if self.replaying
                    && row.state == ActivityState::Waiting
                    && self.replay_newer_display_ids.contains(&row.id)
                {
                    self.projector.remember_activity_enrichment(
                        row.id,
                        PendingActivityEnrichment {
                            summary: row.summary,
                            start_ms: row.start_ms,
                        },
                    );
                    return;
                }
                self.upsert_activity(row);
            }
            ActivityMutation::Settle { id, state, summary } => {
                self.settle_activity_state_or_remember(id, state, summary);
            }
            ActivityMutation::Enrich {
                id,
                summary,
                start_ms,
            } => {
                let mut applied = false;
                if let Some(node) = self.transcript.get_mut(&id) {
                    if let DisplayItem::Activity(row) = &mut node.item {
                        row.summary = summary.clone();
                        if start_ms.is_some() {
                            row.start_ms = start_ms;
                        }
                        applied = true;
                    }
                    self.transcript.touch(&id);
                }
                #[cfg(test)]
                if let Some(row) = self.msgs.iter_mut().find_map(|msg| match msg {
                    Msg::Activity(row) if row.id == id => Some(row),
                    _ => None,
                }) {
                    row.summary = summary.clone();
                    if start_ms.is_some() {
                        row.start_ms = start_ms;
                    }
                    applied = true;
                }
                if applied {
                    self.render.transcript_cache.invalidate();
                } else {
                    self.projector.remember_activity_enrichment(
                        id,
                        PendingActivityEnrichment { summary, start_ms },
                    );
                }
            }
        }
    }

    fn apply_lifecycle_projection(
        &mut self,
        event: &TimelineRecord,
        projection: LifecycleProjection,
    ) {
        match projection {
            LifecycleProjection::TurnStart => {
                self.start_thinking();
                self.session
                    .activity_epoch
                    .get_or_insert_with(std::time::Instant::now);
            }
            LifecycleProjection::TurnEnd { cancelled, outcome } => {
                self.settle_turn(host_event_time(event), cancelled);
                if let Some(mut item) = outcome {
                    if let DisplayItem::Block(block) = &mut item {
                        if block.unit.is_none() {
                            block.unit = Some(self.allocate_copy_unit(&block.copy_source));
                        }
                    }
                    self.insert_transcript_item(item.clone(), None, None);
                    #[cfg(test)]
                    if let DisplayItem::Block(block) = item {
                        if block.tone == DisplayTone::Error {
                            self.msgs.push(Msg::Error {
                                text: block.content,
                            });
                        } else if block.content.contains("token 上限") {
                            self.msgs.push(Msg::Block(block));
                        } else {
                            self.msgs.push(Msg::System {
                                text: block.content,
                            });
                        }
                    }
                    self.render.transcript_cache.invalidate();
                }
            }
            LifecycleProjection::Goal(goal) => self.goal = goal,
            LifecycleProjection::Plan(plan) => self.plan_mode = plan,
            LifecycleProjection::Preset(preset) => {
                let is_newest = match (event.sequence, self.session.current_mode_seq) {
                    (Some(incoming), Some(current)) => incoming >= current,
                    (Some(_), None) | (None, None) => true,
                    (None, Some(_)) => false,
                };
                if is_newest && !preset.is_empty() {
                    self.session.current_mode = Some(preset);
                    self.session.current_mode_seq = event.sequence;
                }
            }
            LifecycleProjection::SessionState(event_type) => {
                self.session_state_events.insert(event_type);
            }
            LifecycleProjection::Ignore => {}
        }
    }

    /// Prepend one typed history page while preserving the current viewport anchor.
    pub fn prepend_host_events(&mut self, events: &[TimelineRecord]) -> usize {
        let existing_owners = self.projector.owned_seqs();
        // Token totals should absorb older pages, but replacement bookkeeping
        // must keep pointing at the newest loaded request.
        let newest_usage_sample = self.session.last_usage_sample;
        #[cfg(test)]
        let saved = std::mem::take(&mut self.msgs);
        let saved_transcript = std::mem::take(&mut self.transcript);
        let saved_transcript_len = saved_transcript.len();
        self.replay_newer_display_ids = saved_transcript
            .nodes()
            .iter()
            .map(|node| node.id().clone())
            .collect();
        self.replaying = true;
        for event in events {
            self.apply_host_event(event);
        }
        self.replaying = false;
        if newest_usage_sample.is_some() {
            self.session.last_usage_sample = newest_usage_sample;
        }
        #[cfg(test)]
        let legacy_added = {
            let mut added = self.msgs.len();
            let dangling = matches!(self.msgs.last(), Some(Msg::Streaming { .. }))
                && matches!(saved.first(), Some(Msg::Assistant { .. }));
            if dangling {
                self.msgs.pop();
                added -= 1;
            }
            self.msgs.extend(saved);
            added
        };
        for node in saved_transcript.nodes() {
            if node
                .surface_seq
                .is_some_and(|seq| self.projector.is_shadowed(seq))
            {
                continue;
            }
            self.transcript.append(node.item.clone(), node.surface_seq);
        }
        let added = self.transcript.len().saturating_sub(saved_transcript_len);
        self.projector
            .shift_selected_owners(&existing_owners, added);
        self.replay_newer_display_ids.clear();
        self.apply_pending_activity_enrichments();
        self.render.transcript_cache.invalidate();
        self.render.transcript_cache.tail_dirty = false;
        self.render.transcript_cache.prepend_anchor =
            Some(self.render.transcript_cache.display_len());
        #[cfg(test)]
        return legacy_added;
        #[cfg(not(test))]
        added
    }
}

/// Capture the settle transition when a file group's last pending item
/// settles: the bullet animates from the breathing color toward umber/red
/// instead of snapping.
#[cfg(test)]
fn settle_group(group: &mut FileGroup, from: Color) {
    if !group.pending() && group.done_since.is_none() {
        group.done_since = Some(std::time::Instant::now());
        group.done_from = Some(from);
    }
}

/// Exit code from the `[exit code: N]` marker in shell tool output.
#[cfg(test)]
fn exit_marker(output: &str) -> i64 {
    for line in output.lines().rev().take(4) {
        if let Some(pos) = line.find("[exit code: ") {
            let rest = &line[pos + "[exit code: ".len()..];
            if let Some(end) = rest.find(']') {
                if let Ok(code) = rest[..end].trim().parse::<i64>() {
                    return code;
                }
            }
        }
    }
    0
}

fn mutation_id(mutation: &ActivityMutation) -> DisplayId {
    match mutation {
        ActivityMutation::Upsert(row) => row.id.clone(),
        ActivityMutation::Settle { id, .. } | ActivityMutation::Enrich { id, .. } => id.clone(),
    }
}

#[cfg(test)]
fn legacy_file_action(label: &str) -> FileAction {
    match label {
        "read" => FileAction::Read,
        "view" => FileAction::View,
        "edit" => FileAction::Edit,
        "replace" => FileAction::Replace,
        "insert" => FileAction::Insert,
        "create" => FileAction::Create,
        _ => FileAction::Read,
    }
}

/// Whether an animation deadline is needed. This is separate from advancing
/// the clock so the event-driven main loop can remain asleep when idle.
pub fn animation_active(state: &RuntimeState, _now: std::time::Instant) -> bool {
    #[cfg(test)]
    if state.transcript.is_empty() && !state.msgs.is_empty() {
        return legacy_animation_active(state);
    }
    state
        .transcript
        .nodes()
        .iter()
        .any(|node| match &node.item {
            DisplayItem::Activity(row) => row.state.is_active(),
            DisplayItem::Block(block) => {
                block.streaming && block.format != TranscriptFormat::Reasoning
            }
            DisplayItem::Composite { activity, .. } => activity.state.is_active(),
            DisplayItem::Thinking(node) => node.row.state.is_active(),
            DisplayItem::Card(_) => false,
        })
        || !state.render.activity_transitions.is_empty()
        || state.session.working
        || state.session.status == AgentStatus::Running
}

/// Advance the breathing/transition animation clock and mark only the public
/// display ranges whose colors can change. Expired transitions submit one
/// final exact-color patch before their sidecar entry is removed.
pub fn tick_spinners(state: &mut RuntimeState, now: std::time::Instant) -> bool {
    #[cfg(test)]
    if state.transcript.is_empty() && !state.msgs.is_empty() {
        return tick_legacy_spinners(state, now);
    }

    let mut any_pending = state.session.working || state.session.status == AgentStatus::Running;
    let mut dirty = Vec::new();
    for (index, node) in state.transcript.nodes().iter().enumerate() {
        let pending = match &node.item {
            DisplayItem::Activity(row) => row.state.is_active(),
            DisplayItem::Block(block) => {
                block.streaming && block.format != TranscriptFormat::Reasoning
            }
            DisplayItem::Composite { activity, .. } => activity.state.is_active(),
            DisplayItem::Thinking(node) => node.row.state.is_active(),
            DisplayItem::Card(_) => false,
        };
        if pending {
            any_pending = true;
            dirty.push(index);
        }
    }

    let transitions = state
        .render
        .activity_transitions
        .iter()
        .map(|(id, transition)| {
            (
                id.clone(),
                now.saturating_duration_since(transition.done_since)
                    .as_millis()
                    >= SETTLE_TRANSITION_MS,
            )
        })
        .collect::<Vec<_>>();
    let mut finalized = Vec::new();
    for (id, expired) in transitions {
        if let Some(index) = state.transcript.position(&id) {
            dirty.push(index);
        }
        if expired {
            finalized.push(id);
        }
    }
    for id in finalized {
        state.render.activity_transitions.remove(&id);
    }

    if any_pending {
        state.session.activity_epoch.get_or_insert(now);
    } else if state.render.activity_transitions.is_empty() {
        state.session.activity_epoch = None;
    }
    dirty.sort_unstable();
    dirty.dedup();
    let animation_changed = !dirty.is_empty();
    for index in dirty {
        state.render.transcript_cache.mark_message_dirty(index);
    }
    any_pending || animation_changed
}

#[cfg(test)]
fn legacy_animation_active(state: &RuntimeState) -> bool {
    let any_pending = state.msgs.iter().any(|message| match message {
        Msg::Tool(card) => card.state == ToolState::Running,
        Msg::FileGroup(group) => group.pending(),
        Msg::Thinking(card) => card.state == ThinkState::Running,
        Msg::Activity(row) => row.state.is_active(),
        Msg::Block(block) => block.streaming && block.format != TranscriptFormat::Reasoning,
        _ => false,
    }) || matches!(state.msgs.last(), Some(Msg::Streaming { .. }))
        || state.session.working
        || state.session.status == AgentStatus::Running;
    let transitioning = state.msgs.iter().any(|message| match message {
        Msg::Tool(card) => card.done_since.is_some() && card.done_from.is_some(),
        Msg::FileGroup(group) => group.done_since.is_some() && group.done_from.is_some(),
        Msg::Thinking(card) => card.done_since.is_some() && card.done_from.is_some(),
        _ => false,
    });
    any_pending || transitioning
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyTransitionTick {
    None,
    Active,
    Finalize,
}

#[cfg(test)]
fn advance_legacy_transition(
    done_since: &mut Option<std::time::Instant>,
    done_from: &mut Option<Color>,
    now: std::time::Instant,
) -> LegacyTransitionTick {
    let (Some(at), Some(_)) = (*done_since, *done_from) else {
        return LegacyTransitionTick::None;
    };
    if now.saturating_duration_since(at).as_millis() < SETTLE_TRANSITION_MS {
        LegacyTransitionTick::Active
    } else {
        *done_from = None;
        LegacyTransitionTick::Finalize
    }
}

#[cfg(test)]
fn tick_legacy_spinners(state: &mut RuntimeState, now: std::time::Instant) -> bool {
    let mut any_pending = state.session.working || state.session.status == AgentStatus::Running;
    let mut animation_changed = false;
    let msg_len = state.msgs.len();
    let mut dirty = Vec::new();
    for (index, message) in state.msgs.iter_mut().enumerate() {
        let (pending, transition) = match message {
            Msg::Tool(card) => (
                card.state == ToolState::Running,
                advance_legacy_transition(&mut card.done_since, &mut card.done_from, now),
            ),
            Msg::FileGroup(group) => (
                group.pending(),
                advance_legacy_transition(&mut group.done_since, &mut group.done_from, now),
            ),
            Msg::Thinking(card) => (
                card.state == ThinkState::Running,
                advance_legacy_transition(&mut card.done_since, &mut card.done_from, now),
            ),
            Msg::Activity(row) => (row.state.is_active(), LegacyTransitionTick::None),
            Msg::Block(block) => (
                block.streaming && block.format != TranscriptFormat::Reasoning,
                LegacyTransitionTick::None,
            ),
            Msg::Streaming { .. } if index + 1 == msg_len => (true, LegacyTransitionTick::None),
            _ => (false, LegacyTransitionTick::None),
        };
        any_pending |= pending;
        if pending || transition != LegacyTransitionTick::None {
            dirty.push(index);
            animation_changed = true;
        }
    }
    if any_pending {
        state.session.activity_epoch.get_or_insert(now);
    } else {
        state.session.activity_epoch = None;
    }
    for index in dirty {
        state.render.transcript_cache.mark_message_dirty(index);
    }
    any_pending || animation_changed
}

fn host_event_time(event: &TimelineRecord) -> u64 {
    event.time_ms.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    })
}

/// Rewrite path-bearing primaries to their workspace-relative display form.
fn relativize_tool_preview(preview: &ToolPreview, workspace: Option<&str>) -> ToolPreview {
    let mut out = preview.clone();
    match &mut out.primary {
        ToolPreviewPrimary::Location { path, .. } => {
            *path = crate::agent::tool::workspace_relative_path(path, workspace);
        }
        ToolPreviewPrimary::Search { path, .. } => {
            if let Some(path) = path.as_mut() {
                *path = crate::agent::tool::workspace_relative_path(path, workspace);
            }
        }
        _ => {}
    }
    out
}

/// Enrich a stored preview seed with settled result facts. `None` keeps the
/// existing call-time preview: primary-only tools (read/view/create/search/
/// generic) never grow a secondary, and a mutation tool without result data
/// retains its call-time fragment.
fn tool_preview_result(
    seed: Option<&ToolPreview>,
    result: Option<(&str, bool, Option<&MutationDiff>, &[MutationHunk])>,
    metrics: Option<ToolMetrics>,
    workspace: Option<&str>,
) -> Option<PreviewContent> {
    match seed {
        Some(seed) => match &seed.primary {
            ToolPreviewPrimary::Command { command, .. } => {
                let (output, truncated, _, _) = result?;
                let metrics = metrics?;
                Some(PreviewContent::Tool(ToolPreview {
                    name: seed.name.clone(),
                    primary: ToolPreviewPrimary::Command {
                        command: command.clone(),
                        metrics,
                    },
                    secondary: Some(ToolPreviewSecondary::Terminal {
                        output: output.to_owned(),
                        truncated,
                    }),
                }))
            }
            _ => None,
        },
        None => {
            let (_, _, diff, hunks) = result?;
            if let Some(diff) = diff {
                Some(PreviewContent::Diff {
                    path: diff
                        .path
                        .as_deref()
                        .map(|path| crate::agent::tool::workspace_relative_path(path, workspace)),
                    source: diff.source.clone(),
                })
            } else if hunks.is_empty() {
                None
            } else {
                Some(PreviewContent::Hunks(
                    hunks
                        .iter()
                        .map(|hunk| MutationHunk {
                            path: hunk.path.as_deref().map(|path| {
                                crate::agent::tool::workspace_relative_path(path, workspace)
                            }),
                            ..hunk.clone()
                        })
                        .collect(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::{
        timeline::{TimelineFact, TimelineRecord},
        tool::{ActivityState as AgentActivityState, ToolActivity, ToolCapability, ToolReference},
    };

    use super::*;

    fn record(sequence: u64, fact: TimelineFact) -> TimelineRecord {
        TimelineRecord {
            sequence: Some(sequence),
            time_ms: Some(sequence * 10),
            surface: None,
            source_sequences: Vec::new(),
            fact,
        }
    }

    fn edit_call() -> ToolActivity {
        ToolActivity {
            id: "edit-1".into(),
            capability: ToolCapability::Edit,
            label: "edit".into(),
            summary: r"G:\repo\src\lib.rs".into(),
            state: AgentActivityState::Running,
            reference: Some(ToolReference::Hunks(vec![MutationHunk {
                path: Some(r"G:\repo\src\lib.rs".into()),
                old: Some("old".into()),
                new: Some("new".into()),
                anchor_line: None,
            }])),
            items: Vec::new(),
            preview: None,
        }
    }

    #[test]
    fn unified_mutation_diff_takes_priority_over_result_hunks() {
        let diff = MutationDiff {
            path: Some(r"G:\repo\src\lib.rs".into()),
            source: "--- src/lib.rs\n+++ src/lib.rs\n-old\n+new\n".into(),
        };
        let hunks = [MutationHunk {
            path: Some(r"G:\repo\src\lib.rs".into()),
            old: Some("requested old".into()),
            new: Some("requested new".into()),
            anchor_line: None,
        }];
        assert!(matches!(
            tool_preview_result(
                None,
                Some(("ok", false, Some(&diff), &hunks)),
                None,
                Some(r"G:\repo")
            ),
            Some(PreviewContent::Diff { path: Some(path), source })
                if path == "src/lib.rs" && source == diff.source
        ));
        assert!(tool_preview_result(None, Some(("ok", false, None, &[])), None, None).is_none());
    }

    #[test]
    fn result_before_call_settles_to_the_same_unified_mutation_diff() {
        let patch = "--- src/lib.rs\n+++ src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let mut state = RuntimeState::default();
        state.session.session_cwd = Some(r"G:\repo".into());
        state.apply_host_event(&record(
            1,
            TimelineFact::ToolResult {
                activity_id: "edit-1".into(),
                output: "ok".into(),
                state: AgentActivityState::Success,
                output_truncated: false,
                mutation_diff: Some(MutationDiff {
                    path: None,
                    source: patch.into(),
                }),
                mutation_hunks: Vec::new(),
            },
        ));
        state.apply_host_event(&record(2, TimelineFact::ToolCall(edit_call())));

        assert!(state.timeline.preview_refs.values().any(|preview| matches!(
            preview,
            PreviewRef::Inline {
                key: PreviewKey(key),
                content: PreviewContent::Diff { source, .. },
                ..
            } if key == "tool:edit-1" && source == patch
        )));
    }
}
