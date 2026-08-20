//! Session-event projection into renderable message cards (design §3.2/§3.3).
//!
//! The bridge forwards raw session events; this module folds them into a
//! small owned message model. Tool cards are created on `tool/call` and
//! finalized on `tool/result` (D21/D22); assistant text streams in through
//! `assistant/chunk` and is replaced by the assembled `assistant/message`.

use serde_json::Value;

#[cfg(test)]
use ratatui::style::Color;

#[cfg(test)]
use crate::config::Theme;
use crate::protocol::{ClientMessage, HostEvent, CLIENT_REPLAY_EVENT_CAP};
pub use e_tui::app::{
    breathing_color, lerp_color, settle_color, BREATH_CYCLE_MS, SETTLE_TRANSITION_MS,
};
#[cfg(test)]
use e_tui::display::ContentCard;
use e_tui::display::{
    ActivityRow, ActivityState, CardRole, DisplayId, DisplayItem, DisplayTone, ThinkingNode,
    TranscriptBlock, TranscriptFormat,
};
use e_tui::projection::{
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
use e_tui::render::RenderLine;
use e_tui::render::RenderOptions;
use e_tui::{
    agent::timeline::{SurfaceOperation, TimelineFact, TimelineRecord, TokenUsage},
    preview::{MutationHunk, ToolMetrics, ToolPreview, ToolPreviewPrimary, ToolPreviewSecondary},
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
    /// Event time of the tool/call (duration base, D28 toggle).
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
    Block(e_tui::display::TranscriptBlock),
    /// Shared padded content-card display surface.
    Card(e_tui::display::ContentCard),
    /// Shared status-bearing activity display surface.
    Activity(e_tui::display::ActivityRow),
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
    pub fn from_node(node: &e_tui::display::ThinkingNode) -> Self {
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

pub use e_tui::{
    interaction::ApprovalCard, question::QuestionBatch, NewConversationDraft,
    SessionStatus as AgentStatus,
};

pub struct AppState {
    /// Kernel-neutral application root. Existing field-style callers are
    /// temporarily forwarded through `Deref` while lifecycle owners move.
    pub tui: TuiApp,
    #[cfg(test)]
    pub msgs: Vec<Msg>,
}

impl std::ops::Deref for AppState {
    type Target = TuiApp;

    fn deref(&self) -> &Self::Target {
        &self.tui
    }
}

impl std::ops::DerefMut for AppState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tui
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            tui: TuiApp::default(),
            #[cfg(test)]
            msgs: Vec::new(),
        }
    }
}

impl AppState {
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
    /// caller must send it immediately instead (the agent is idle).
    pub fn enqueue_or_immediate(&mut self, text: &str) -> bool {
        if self.session.status == AgentStatus::Running {
            self.interaction.queue.push(text.to_string());
            false
        } else {
            true
        }
    }

    /// The agent went idle: pop the next queued prompt for auto-dispatch,
    /// one at a time (each dispatch keeps the agent busy until it returns
    /// to idle again).
    pub fn take_next_queued(&mut self) -> Option<String> {
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

impl AppState {
    /// Drop the whole transcript (used when attaching to another session).
    pub fn reset_transcript(&mut self) {
        self.transcript.clear();
        self.render.markdown_layout.clear();
        self.render.activity_transitions.clear();
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

impl AppState {
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
    pub fn materialize_new_conversation(&mut self, text: String) -> Option<ClientMessage> {
        let draft = self.session.new_conversation.as_mut()?;
        if draft.pending_input.is_some() {
            return None;
        }
        draft.pending_input = Some(text.clone());
        draft.notice = Some("正在创建新对话…".into());
        Some(ClientMessage::NewInput {
            mode: draft.mode.clone(),
            text,
        })
    }

    pub fn restore_new_conversation_input(&mut self) -> Option<String> {
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

    /// Apply one bridge message payload (welcome / snapshot / event / status).
    pub fn apply(&mut self, kind: &str, data: &Value) {
        match kind {
            "welcome" => {
                let new_id = data
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(String::from);
                if let (Some(prev), Some(next)) = (&self.session.session_id, &new_id) {
                    if prev != next {
                        self.reset_transcript();
                        // A pending question belongs to the old session.
                        self.interaction.question = None;
                    }
                }
                self.session.session_id = new_id;
                // Only a real bridge welcome can commit/abandon a local draft.
                self.session.new_conversation = None;
                self.session.session_title =
                    data.get("title").and_then(Value::as_str).map(String::from);
                self.session.session_cwd =
                    data.get("cwd").and_then(Value::as_str).map(String::from);
                self.session.status =
                    if data.get("status").and_then(Value::as_str) == Some("running") {
                        AgentStatus::Running
                    } else {
                        AgentStatus::Idle
                    };
                // Snapshot replay refines this: activity events clear it,
                // a mid-thought tail keeps it.
                self.session.working = self.session.status == AgentStatus::Running;
                self.session.provider = data
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(String::from);
                self.session.model = data.get("model").and_then(Value::as_str).map(String::from);
                self.session.current_mode = data
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or_else(|| Some(self.config.default_mode.clone()));
                self.session.current_mode_seq = None;
            }
            "snapshot" => {
                let events: Vec<TimelineRecord> = data
                    .get("events")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned()
                    .map(HostEvent::from_value)
                    .map(crate::bridge::adapter::normalize_host_event)
                    .collect();
                let bridge_truncated = data
                    .get("truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.apply_snapshot(&events, bridge_truncated);
            }
            "event" => self.apply_event(data),
            "status" => {
                self.session.status =
                    if data.get("status").and_then(Value::as_str) == Some("running") {
                        AgentStatus::Running
                    } else {
                        AgentStatus::Idle
                    };
                if self.session.status == AgentStatus::Idle {
                    self.stop_thinking();
                }
            }
            _ => {}
        }
    }

    /// Apply a typed replay window. The client replay budget is generated
    /// from the canonical wire contract and protects the local render cache.
    pub fn apply_snapshot(&mut self, events: &[TimelineRecord], bridge_truncated: bool) {
        let mut surface: Vec<&TimelineRecord> = events
            .iter()
            .filter(|event| event.is_replay_relevant())
            .collect();
        let truncated = bridge_truncated || surface.len() > CLIENT_REPLAY_EVENT_CAP;
        if truncated {
            surface = surface.split_off(surface.len().saturating_sub(CLIENT_REPLAY_EVENT_CAP));
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

    /// Compatibility entry for tests and persisted JSON callers. Raw host
    /// values are translated once at the protocol boundary before reduction.
    pub fn apply_event(&mut self, event: &Value) {
        self.apply_host_event(&crate::bridge::adapter::normalize_host_event(
            HostEvent::from_value(event.clone()),
        ));
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
                        e_tui::display::DisplayItem::Block(mut block) => {
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
                        e_tui::display::DisplayItem::Card(mut card) => {
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
                        e_tui::display::DisplayItem::Activity(row) => self.upsert_activity(row),
                        e_tui::display::DisplayItem::Thinking(mut node) => {
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
                        e_tui::display::DisplayItem::Composite {
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

    fn upsert_activity(&mut self, mut row: e_tui::display::ActivityRow) {
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

        if incoming.streaming {
            let is_tail = self.transcript.position(&id) == self.transcript.len().checked_sub(1);
            #[cfg(test)]
            match self.msgs.last_mut() {
                Some(Msg::Streaming { text }) => text.push_str(&incoming.content),
                _ => self.msgs.push(Msg::Streaming {
                    text: incoming.content,
                }),
            }
            if existed && is_tail {
                self.render.transcript_cache.mark_tail_dirty();
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
        self.render.transcript_cache.invalidate();
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
                            is_error: !matches!(state, e_tui::agent::ActivityState::Success),
                            output_truncated: *output_truncated,
                            time_ms: now_ms,
                            surface_seq: event.sequence,
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
                    .map(|item| e_tui::agent::tool::ToolItem {
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
                    Some((output, *output_truncated, mutation_hunks)),
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
                !matches!(state, e_tui::agent::ActivityState::Success),
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

    /// Compatibility entry for persisted JSON and model-level tests.
    pub fn prepend_events(&mut self, events: &[Value]) -> usize {
        let typed: Vec<TimelineRecord> = events
            .iter()
            .cloned()
            .map(HostEvent::from_value)
            .map(crate::bridge::adapter::normalize_host_event)
            .collect();
        self.prepend_host_events(&typed)
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
pub fn animation_active(state: &AppState, _now: std::time::Instant) -> bool {
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
pub fn tick_spinners(state: &mut AppState, now: std::time::Instant) -> bool {
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
fn legacy_animation_active(state: &AppState) -> bool {
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
fn tick_legacy_spinners(state: &mut AppState, now: std::time::Instant) -> bool {
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
            *path = e_tui::agent::tool::workspace_relative_path(path, workspace);
        }
        ToolPreviewPrimary::Search { path, .. } => {
            if let Some(path) = path.as_mut() {
                *path = e_tui::agent::tool::workspace_relative_path(path, workspace);
            }
        }
        _ => {}
    }
    out
}

/// Enrich a stored preview seed with settled result facts. `None` keeps the
/// existing call-time preview: primary-only tools (read/view/create/search/
/// generic) never grow a secondary, and a mutation tool without result hunks
/// retains its call-time fragment.
fn tool_preview_result(
    seed: Option<&ToolPreview>,
    result: Option<(&str, bool, &[MutationHunk])>,
    metrics: Option<ToolMetrics>,
    workspace: Option<&str>,
) -> Option<PreviewContent> {
    match seed {
        Some(seed) => match &seed.primary {
            ToolPreviewPrimary::Command { command, .. } => {
                let (output, truncated, _) = result?;
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
            let (_, _, hunks) = result?;
            if hunks.is_empty() {
                None
            } else {
                Some(PreviewContent::Hunks(
                    hunks
                        .iter()
                        .map(|hunk| MutationHunk {
                            path: hunk.path.as_deref().map(|path| {
                                e_tui::agent::tool::workspace_relative_path(path, workspace)
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
    use super::*;

    fn event(ty: &str, data: Value) -> Value {
        serde_json::json!({ "type": ty, "seq": 1, "time": 0, "data": data })
    }

    fn event_seq(ty: &str, seq: u64, data: Value) -> Value {
        serde_json::json!({ "type": ty, "seq": seq, "time": 0, "data": data })
    }

    #[test]
    fn user_message_verbatim() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "原样展示 # not markdown"}],
                "source": {"kind": "user"}
            }),
        ));
        assert!(matches!(&s.msgs[0], Msg::User { text } if text == "原样展示 # not markdown"));
        assert!(matches!(
            &s.transcript.nodes()[0].item,
            DisplayItem::Card(card)
                if card.role == CardRole::User && card.content == "原样展示 # not markdown"
        ));
    }

    #[test]
    fn synthetic_context_is_dim() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "injected"}],
                "source": {"kind": "plugin", "plugin": "x"}
            }),
        ));
        assert!(matches!(&s.msgs[0], Msg::System { .. }));
    }

    #[test]
    fn context_injection_previews_as_muted_markdown() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "**bold** injected"}],
                "source": {"kind": "context", "form": "instructions"}
            }),
        ));
        let node = &s.transcript.nodes()[0];
        let reference = s
            .preview_refs
            .get(node.id())
            .expect("context card owns a muted-markdown preview");
        assert!(matches!(
            reference,
            e_tui::preview::PreviewRef::Inline {
                content: e_tui::preview::PreviewContent::MutedMarkdown(text),
                ..
            } if text == "**bold** injected"
        ));
    }

    #[test]
    fn user_message_precedes_the_optimistic_thinking_card() {
        let mut s = AppState::default();
        // The client shows this placeholder the moment the user sends a
        // message, before the bridge echoes the `user/message` event.
        s.start_thinking();
        assert!(matches!(s.msgs.last(), Some(Msg::Thinking(_))));
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "你好"}],
                "source": {"kind": "user"}
            }),
        ));
        assert_eq!(s.msgs.len(), 2);
        assert!(matches!(&s.msgs[0], Msg::User { text } if text == "你好"));
        assert!(matches!(&s.msgs[1], Msg::Thinking(_)));
    }

    #[test]
    fn chunks_stream_then_assemble() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "index": 0, "text": "你好"}
            }),
        ));
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "index": 0, "text": "，世界"}
            }),
        ));
        assert!(matches!(&s.msgs[0], Msg::Streaming { text } if text == "你好，世界"));
        s.apply_event(&event(
            "assistant/message",
            serde_json::json!({
                "message": {"content": [{"type": "text", "text": "你好，世界"}]}
            }),
        ));
        assert_eq!(s.msgs.len(), 1);
        assert!(matches!(&s.msgs[0], Msg::Assistant { text, .. } if text == "你好，世界"));
        assert_eq!(s.transcript.len(), 1);
        assert!(matches!(
            &s.transcript.nodes()[0].item,
            DisplayItem::Block(block)
                if block.format == TranscriptFormat::Markdown
                    && block.content == "你好，世界"
                    && !block.streaming
        ));
        assert!(s
            .render
            .markdown_layout
            .unit_start(s.transcript.nodes()[0].id())
            .is_some());
    }

    #[test]
    fn command_result_enriches_preview_with_metrics_and_output() {
        let mut s = AppState::default();
        s.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "c1", "name": "bash",
                "arguments": "{\"command\": \"npm run build\"}"
            }),
        ));
        s.apply_event(&event_seq(
            "tool/result",
            2,
            serde_json::json!({
                "message": {"content": [{
                    "toolCallId": "c1",
                    "content": [{"type": "text", "text": "line1\nline2"}]
                }]}
            }),
        ));
        let reference = s
            .preview_refs
            .get(s.transcript.nodes()[0].id())
            .expect("command row owns a preview");
        assert!(matches!(
            reference,
            e_tui::preview::PreviewRef::Inline {
                content: e_tui::preview::PreviewContent::Tool(preview),
                ..
            } if preview.name == "bash"
                && matches!(
                    &preview.primary,
                    e_tui::preview::ToolPreviewPrimary::Command { command, metrics }
                        if command == "npm run build"
                            && metrics.output_lines == 2
                            && metrics.duration_ms.is_some()
                )
                && matches!(
                    &preview.secondary,
                    Some(e_tui::preview::ToolPreviewSecondary::Terminal { output, truncated: false })
                        if output == "line1\nline2"
                )
        ));
    }

    #[test]
    fn read_result_keeps_primary_only_location_preview() {
        let mut s = AppState::default();
        s.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "c1", "name": "str_replace_editor",
                "arguments": "{\"command\":\"view\",\"path\":\"src/main.rs\"}"
            }),
        ));
        s.apply_event(&event_seq(
            "tool/result",
            2,
            serde_json::json!({
                "message": {"content": [{
                    "toolCallId": "c1",
                    "content": [{"type": "text", "text": "fn main() {}"}]
                }]}
            }),
        ));
        let reference = s
            .preview_refs
            .get(s.transcript.nodes()[0].id())
            .expect("read row owns a preview");
        assert!(matches!(
            reference,
            e_tui::preview::PreviewRef::Inline {
                content: e_tui::preview::PreviewContent::Tool(preview),
                ..
            } if preview.secondary.is_none()
                && matches!(
                    &preview.primary,
                    e_tui::preview::ToolPreviewPrimary::Location { path, .. }
                        if path == "src/main.rs"
                )
        ));
    }

    #[test]
    fn edit_result_meta_diffs_replace_pending_hunk() {
        let mut s = AppState::default();
        s.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "c1", "name": "edit",
                "arguments": "{\"file_path\":\"a.rs\",\"old_string\":\"hello\",\"new_string\":\"hi\"}"
            }),
        ));
        s.apply_event(&event_seq(
            "tool/result",
            2,
            serde_json::json!({
                "message": {"content": [{
                    "toolCallId": "c1",
                    "content": [{"type": "text", "text": "ok"}]
                }]},
                "meta": {"diffs": [
                    {"path": "a.rs", "oldText": "hello\nworld", "newText": "hi\nthere"}
                ]}
            }),
        ));
        let reference = s
            .preview_refs
            .get(s.transcript.nodes()[0].id())
            .expect("edit row owns a preview");
        assert!(matches!(
            reference,
            e_tui::preview::PreviewRef::Inline {
                content: e_tui::preview::PreviewContent::Hunks(hunks),
                ..
            } if hunks.len() == 1
                && hunks[0].old.as_deref() == Some("hello\nworld")
                && hunks[0].new.as_deref() == Some("hi\nthere")
        ));
    }

    #[test]
    fn str_replace_result_keeps_requested_hunk() {
        let mut s = AppState::default();
        s.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "c1", "name": "str_replace_editor",
                "arguments": "{\"command\":\"str_replace\",\"path\":\"a.rs\",\"old_str\":\"hello\",\"new_str\":\"hi\"}"
            }),
        ));
        s.apply_event(&event_seq(
            "tool/result",
            2,
            serde_json::json!({
                "message": {"content": [{
                    "toolCallId": "c1",
                    "content": [{"type": "text", "text": "ok"}]
                }]}
            }),
        ));
        let reference = s
            .preview_refs
            .get(s.transcript.nodes()[0].id())
            .expect("str_replace row owns a preview");
        assert!(matches!(
            reference,
            e_tui::preview::PreviewRef::Inline {
                content: e_tui::preview::PreviewContent::Hunks(hunks),
                ..
            } if hunks.len() == 1
                && hunks[0].old.as_deref() == Some("hello")
                && hunks[0].new.as_deref() == Some("hi")
        ));
    }

    #[test]
    fn tool_card_lifecycle_exit_code() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "tool/call",
            serde_json::json!({
                "callId": "c1", "name": "bash",
                "arguments": "{\"command\": \"npm run build\"}"
            }),
        ));
        assert!(matches!(&s.msgs[0], Msg::Tool(card)
            if card.summary == "npm run build" && card.state == ToolState::Running));
        s.apply_event(&event(
            "tool/result",
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result",
                    "toolCallId": "c1",
                    "content": [{"type": "text", "text": "line1\nline2\n[exit code: 1]"}]
                }]}
            }),
        ));
        assert!(matches!(&s.msgs[0], Msg::Tool(card)
            if card.state == ToolState::Done { ok: false, lines: 3, lines_truncated: false, duration_ms: 0 }));
        assert!(matches!(
            &s.transcript.nodes()[0].item,
            DisplayItem::Activity(row) if row.state == ActivityState::Failure
        ));
    }

    #[test]
    fn read_tool_previews_workspace_relative_path_without_resolving() {
        let mut state = AppState::default();
        state.session.session_cwd = Some(r"G:\workspace".into());
        state.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "view-1",
                "name": "str_replace_editor",
                "arguments": "{\"command\":\"view\",\"path\":\"G:\\\\workspace\\\\src\\\\main.rs\"}"
            }),
        ));
        assert!(
            state.take_actions().is_empty(),
            "read previews are inline paths and must not request file resolution"
        );
        let reference = state
            .preview_refs
            .get(state.transcript.nodes()[0].id())
            .expect("read row owns a preview reference");
        assert!(matches!(
            reference,
            e_tui::preview::PreviewRef::Inline {
                content: e_tui::preview::PreviewContent::Tool(preview),
                ..
            } if preview.name == "view"
                && matches!(
                    &preview.primary,
                    e_tui::preview::ToolPreviewPrimary::Location { path, lines: None }
                        if path == "src/main.rs"
                )
        ));
        let stored = &state.tool_items[state.transcript.nodes()[0].id()][0].reference;
        assert!(
            matches!(stored, e_tui::agent::tool::ToolReference::Lines { path, .. } if path == "src/main.rs"),
            "stored tool items are relativized too, so Reading items agree"
        );
    }

    #[test]
    fn read_tool_preview_keeps_absolute_path_outside_the_workspace() {
        let mut state = AppState::default();
        state.session.session_cwd = Some(r"G:\workspace".into());
        state.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "view-1",
                "name": "str_replace_editor",
                "arguments": "{\"command\":\"view\",\"path\":\"C:\\\\other\\\\lib.rs\"}"
            }),
        ));
        let reference = state
            .preview_refs
            .get(state.transcript.nodes()[0].id())
            .expect("read row owns a preview reference");
        assert!(matches!(
            reference,
            e_tui::preview::PreviewRef::Inline {
                content: e_tui::preview::PreviewContent::Tool(preview),
                ..
            } if matches!(
                &preview.primary,
                e_tui::preview::ToolPreviewPrimary::Location { path, .. }
                    if path == "C:/other/lib.rs"
            )
        ));
    }

    #[test]
    fn read_edit_calls_merge_into_one_group() {
        let mut s = AppState::default();
        for (seq, id, name, path) in [
            (1, "r1", "read", "src/input.rs"),
            (2, "r2", "read", "src/foo.rs"),
            (3, "e1", "edit", "src/ui.rs"),
        ] {
            s.apply_event(&event_seq(
                "tool/call",
                seq,
                serde_json::json!({
                    "callId": id, "name": name,
                    "arguments": format!("{{\"file_path\": \"{path}\"}}")
                }),
            ));
        }
        assert_eq!(s.msgs.len(), 1, "consecutive read/edit calls merge");
        let Msg::FileGroup(group) = &s.msgs[0] else {
            panic!("FileGroup expected")
        };
        assert_eq!(
            group
                .items
                .iter()
                .filter(|item| item.action == FileAction::Read)
                .count(),
            2
        );
        assert_eq!(
            group
                .items
                .iter()
                .filter(|item| item.action == FileAction::Edit)
                .count(),
            1
        );
        assert_eq!(s.transcript.len(), 1);
        assert!(matches!(
            &s.transcript.nodes()[0].item,
            DisplayItem::Activity(row)
                if row.label == "read" && row.continuations.len() == 2
        ));
        // A non-file tool breaks the group; the next read starts a new one.
        s.apply_event(&event_seq(
            "tool/call",
            4,
            serde_json::json!({
                "callId": "b1", "name": "bash", "arguments": "{}"
            }),
        ));
        s.apply_event(&event_seq(
            "tool/call",
            5,
            serde_json::json!({
                "callId": "r3", "name": "read",
                "arguments": "{\"file_path\": \"src/next.rs\"}"
            }),
        ));
        assert_eq!(s.msgs.len(), 3);
        assert!(matches!(&s.msgs[2], Msg::FileGroup(g)
            if g.items.len() == 1 && g.items[0].action == FileAction::Read));
    }

    #[test]
    fn str_replace_editor_folds_operations_but_keeps_create_standalone() {
        let mut s = AppState::default();
        s.session.session_cwd = Some(r"G:\workspace".into());
        for (seq, id, command, path) in [
            (1, "v1", "view", r"G:\workspace\src\a.rs"),
            (2, "r1", "str_replace", r"G:\workspace\src\b.rs"),
            (3, "i1", "insert", r"G:\workspace\src\c.rs"),
        ] {
            s.apply_event(&event_seq(
                "tool/call",
                seq,
                serde_json::json!({
                    "callId": id,
                    "name": "str_replace_editor",
                    "arguments": serde_json::json!({
                        "command": command, "path": path
                    }).to_string()
                }),
            ));
        }
        let Msg::FileGroup(group) = &s.msgs[0] else {
            panic!("editor operations should use FileGroup")
        };
        assert_eq!(
            group
                .items
                .iter()
                .map(|item| (item.action, item.file.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (FileAction::View, "src/a.rs"),
                (FileAction::Replace, "src/b.rs"),
                (FileAction::Insert, "src/c.rs"),
            ]
        );

        s.apply_event(&event_seq(
            "tool/call",
            4,
            serde_json::json!({
                "callId": "c1",
                "name": "str_replace_editor",
                "arguments": serde_json::json!({
                    "command": "create", "path": r"G:\workspace\src\new.rs"
                }).to_string()
            }),
        ));
        assert!(matches!(&s.msgs[1], Msg::Tool(card)
            if card.name == "create" && card.summary == "src/new.rs"));
    }

    #[test]
    fn tool_result_settles_read_and_edit() {
        let mut s = AppState::default();
        s.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "r1", "name": "read",
                "arguments": "{\"file_path\": \"a.rs\"}"
            }),
        ));
        s.apply_event(&event_seq(
            "tool/call",
            2,
            serde_json::json!({
                "callId": "e1", "name": "edit",
                "arguments": "{\"file_path\": \"b.rs\"}"
            }),
        ));
        s.apply_event(&event_seq(
            "tool/result",
            3,
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result", "toolCallId": "r1",
                    "content": [{"type": "text", "text": "content"}]
                }]}
            }),
        ));
        s.apply_event(&event_seq(
            "tool/result",
            4,
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result", "toolCallId": "e1",
                    "content": [{"type": "text", "text": "[exit code: 1]"}]
                }]}
            }),
        ));
        assert!(
            matches!(&s.msgs[0], Msg::FileGroup(g)
                if g.items[0].ok == Some(true) && g.items[1].ok == Some(false)),
            "read ok, edit failed"
        );
    }

    #[test]
    fn file_group_line_count_tracks_the_renderer() {
        let item = |action: FileAction, id: &str, ok: Option<bool>| FileItem {
            action,
            call_id: id.into(),
            file: format!("{id}.rs"),
            ok,
        };
        // All settled: folded ok line + one line per failure.
        let group = FileGroup {
            items: vec![
                item(FileAction::Read, "a", Some(true)),
                item(FileAction::View, "b", Some(false)),
                item(FileAction::Replace, "c", Some(false)),
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        assert_eq!(file_group_line_count(&group), 3);
        // Reads pending: breathing read/view line + settled failures.
        let group = FileGroup {
            items: vec![
                item(FileAction::Read, "a", None),
                item(FileAction::View, "b", Some(false)),
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        assert_eq!(file_group_line_count(&group), 2);
        // Writes pending: ok reads line + failed reads + breathing writes + failures.
        let group = FileGroup {
            items: vec![
                item(FileAction::Read, "a", Some(true)),
                item(FileAction::View, "b", Some(false)),
                item(FileAction::Replace, "c", None),
                item(FileAction::Insert, "d", Some(false)),
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        assert_eq!(file_group_line_count(&group), 4);
        // All failed: no folded line, one row per failure.
        let group = FileGroup {
            items: vec![
                item(FileAction::Read, "a", Some(false)),
                item(FileAction::Edit, "c", Some(false)),
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        assert_eq!(file_group_line_count(&group), 2);
    }

    #[test]
    fn working_tracks_turn_activity() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        assert!(s.session.working, "Working after turn/start");
        s.apply_event(&event(
            "tool/call",
            serde_json::json!({
                "callId": "c1", "name": "bash", "arguments": "{}"
            }),
        ));
        assert!(!s.session.working, "tool call is visible activity");
        s.apply_event(&event(
            "tool/result",
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result", "toolCallId": "c1",
                    "content": [{"type": "text", "text": "ok"}]
                }]}
            }),
        ));
        assert!(s.session.working, "model works again after a tool result");
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "hi"}
            }),
        ));
        assert!(!s.session.working);
        s.apply_event(&event("turn/end", serde_json::json!({})));
        assert!(!s.session.working);
        // Idle status clears a stale flag.
        s.session.working = true;
        s.apply("status", &serde_json::json!({"status": "idle"}));
        assert!(!s.session.working);
        // The echo of the user's own message keeps Working while the agent
        // runs, and clears it when idle (no work follows the echo).
        s.session.working = true;
        s.session.status = AgentStatus::Running;
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }),
        ));
        assert!(s.session.working, "running echo keeps Working alive");
        s.session.status = AgentStatus::Idle;
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }),
        ));
        assert!(!s.session.working, "idle echo clears Working");
    }

    /// Esc interrupt regression: a turn ending without result events must
    /// settle every still-running indicator so no bullet keeps breathing.
    #[test]
    fn interrupt_settles_running_tool_card() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        s.apply_event(&event(
            "tool/call",
            serde_json::json!({
                "callId": "c1", "name": "bash", "arguments": "{}"
            }),
        ));
        assert!(
            matches!(&s.msgs[1], Msg::Tool(card) if card.state == ToolState::Running),
            "tool running before the interrupt"
        );
        s.apply_event(&event(
            "turn/end",
            serde_json::json!({
                "reason": {"kind": "aborted"}
            }),
        ));
        assert!(
            matches!(
                s.msgs.iter().find(|m| matches!(m, Msg::Tool(_))),
                Some(Msg::Tool(card)) if matches!(card.state, ToolState::Done { ok: false, .. })
            ),
            "interrupted tool settles as failed"
        );
        assert!(!s.session.working);
        assert!(
            s.msgs
                .iter()
                .any(|m| matches!(m, Msg::System { text } if text == "（已中断）")),
            "abort notice present"
        );
        // No running/pending/streaming rows remain, and once the settle
        // transition completes the animation clock stops entirely.
        let any_running = s.msgs.iter().any(|m| match m {
            Msg::Tool(card) => card.state == ToolState::Running,
            Msg::FileGroup(group) => group.pending(),
            Msg::Thinking(card) => card.state == ThinkState::Running,
            Msg::Streaming { .. } => true,
            _ => false,
        });
        assert!(!any_running, "nothing left breathing");
        for transition in s.render.activity_transitions.values_mut() {
            transition.done_since = std::time::Instant::now() - std::time::Duration::from_secs(1);
        }
        for m in s.msgs.iter_mut() {
            let age = |d: &mut Option<std::time::Instant>| {
                *d = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
            };
            match m {
                Msg::Tool(card) => age(&mut card.done_since),
                Msg::FileGroup(group) => age(&mut group.done_since),
                Msg::Thinking(card) => age(&mut card.done_since),
                _ => {}
            }
        }
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "expired settle transition emits its final target-color patch"
        );
        s.render.transcript_cache.dirty_messages.clear();
        assert!(
            !tick_spinners(&mut s, std::time::Instant::now()),
            "animation clock stops after the final patch"
        );
    }

    /// Esc interrupt mid-stream: the dangling streaming tail is finalized
    /// into an assistant message so its breathing bullet stops.
    #[test]
    fn interrupt_finalizes_streaming_tail() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "部分输出"}
            }),
        ));
        assert!(matches!(s.msgs.last(), Some(Msg::Streaming { .. })));
        s.apply_event(&event(
            "turn/end",
            serde_json::json!({
                "reason": {"kind": "aborted"}
            }),
        ));
        assert!(
            s.msgs.iter().any(|m| matches!(m, Msg::Assistant { .. })),
            "dangling streaming finalized into an assistant message"
        );
        assert!(
            !s.msgs.iter().any(|m| matches!(m, Msg::Streaming { .. })),
            "no streaming rows left breathing"
        );
        assert!(
            matches!(s.msgs.last(), Some(Msg::System { text }) if text == "（已中断）"),
            "abort notice follows"
        );
    }

    /// Esc interrupt while a read/edit group and a thinking phase are open:
    /// both settle instead of breathing forever.
    #[test]
    fn interrupt_settles_groups_and_thinking() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        s.apply_event(&event(
            "tool/call",
            serde_json::json!({
                "callId": "r1", "name": "read", "arguments": "{\"file\": \"a.txt\"}"
            }),
        ));
        s.apply_event(&event(
            "tool/result",
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result", "toolCallId": "r1",
                    "content": [{"type": "text", "text": "ok"}]
                }]}
            }),
        ));
        assert!(
            s.msgs
                .iter()
                .any(|m| matches!(m, Msg::Thinking(card) if card.state == ThinkState::Running)),
            "thinking runs after the read result"
        );
        // Second read is still pending when the interrupt lands.
        s.apply_event(&event(
            "tool/call",
            serde_json::json!({
                "callId": "r2", "name": "read", "arguments": "{\"file\": \"b.txt\"}"
            }),
        ));
        s.apply_event(&event(
            "turn/end",
            serde_json::json!({
                "reason": {"kind": "aborted"}
            }),
        ));
        let group = s
            .msgs
            .iter()
            .find_map(|m| match m {
                Msg::FileGroup(group) => Some(group),
                _ => None,
            })
            .expect("file group present");
        assert!(!group.pending(), "interrupted read settles");
        assert!(
            group.items.iter().any(|i| i.ok == Some(false)),
            "unresolved read marked failed"
        );
        assert!(
            s.msgs
                .iter()
                .all(|m| !matches!(m, Msg::Thinking(card) if card.state == ThinkState::Running)),
            "thinking rows settled"
        );
        assert!(!s.session.working);
    }

    #[test]
    fn thinking_card_tracks_phases() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        assert!(
            matches!(s.msgs.last(), Some(Msg::Thinking(card)) if card.state == ThinkState::Running),
            "turn/start shows a running Thinking row"
        );
        // Visible activity settles it green.
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "hi"}
            }),
        ));
        assert!(
            matches!(s.msgs.first(), Some(Msg::Thinking(card)) if card.state == ThinkState::Done),
            "activity settles the Thinking row green"
        );
        // A tool result starts a fresh phase.
        s.apply_event(&event(
            "tool/result",
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result", "toolCallId": "c1",
                    "content": [{"type": "text", "text": "ok"}]
                }]}
            }),
        ));
        assert!(
            matches!(s.msgs.last(), Some(Msg::Thinking(card)) if card.state == ThinkState::Running),
            "model thinks again after a tool result"
        );
        // The turn end settles the last phase.
        s.apply_event(&event("turn/end", serde_json::json!({})));
        assert!(
            matches!(s.msgs.last(), Some(Msg::Thinking(card)) if card.state == ThinkState::Done),
            "turn/end settles the Thinking row"
        );
        assert!(!s.session.working);
    }

    /// Thinking output (reasoning) is collapsed into the breathing
    /// `• Thinking...` row: reasoning chunks keep the row running until real
    /// answer text arrives, at which point it settles green.
    #[test]
    fn reasoning_keeps_thinking_breathing_until_text_arrives() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        assert!(
            matches!(s.msgs.last(), Some(Msg::Thinking(card)) if card.state == ThinkState::Running),
            "turn/start shows a running Thinking row"
        );
        // A reasoning-only chunk keeps the Thinking row breathing and stores
        // the (hidden) reasoning block behind it.
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "reasoning-delta", "text": "thinking out loud"}
            }),
        ));
        assert!(s.session.working, "still working while reasoning streams");
        assert!(
            matches!(
                s.msgs.iter().rev().find(|m| matches!(m, Msg::Thinking(_))),
                Some(Msg::Thinking(card)) if card.state == ThinkState::Running
            ),
            "reasoning keeps the Thinking row running"
        );
        assert!(
            matches!(
                s.msgs.iter().rev().find(|m| matches!(m, Msg::Thinking(_))),
                Some(Msg::Thinking(card))
                    if card.state == ThinkState::Running
                        && card.content == "thinking out loud"
            ),
            "reasoning accumulates into the merged Thinking node rather than dropping"
        );
        // Real answer text settles the Thinking row green.
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "hi"}
            }),
        ));
        assert!(
            matches!(
                s.msgs.iter().rev().find(|m| matches!(m, Msg::Thinking(_))),
                Some(Msg::Thinking(card)) if card.state == ThinkState::Done
            ),
            "answer text settles the Thinking row green"
        );
    }

    /// In `Lines` and `Full` modes the reasoning text becomes visible. The
    /// breathing `Thinking...` indicator still owns the work lifecycle at the
    /// model level; the display layer supersedes (hides) the row once the
    /// reasoning content next to it is rendered.
    #[test]
    fn visible_reasoning_modes_keep_the_thinking_indicator() {
        let mut s = AppState::default();
        s.config.thinking_display = "full".into();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "reasoning-delta", "text": "one\ntwo\nthree"}
            }),
        ));
        assert!(
            matches!(
                s.msgs.iter().find(|m| matches!(m, Msg::Thinking(_))),
                Some(Msg::Thinking(card)) if card.state == ThinkState::Running
            ),
            "visible reasoning still has a running Thinking indicator"
        );
        assert!(
            matches!(
                s.msgs.iter().find(|m| matches!(m, Msg::Thinking(_))),
                Some(Msg::Thinking(card))
                    if card.state == ThinkState::Running
                        && card.content == "one\ntwo\nthree"
                        && card.unit.is_some()
            ),
            "visible reasoning lives inside the running Thinking node with a copy unit"
        );
    }

    /// Live reasoning merges into the `start_thinking` node; the node must
    /// acquire a copy unit on the first chunk so the accumulated content is
    /// selectable/copyable instead of silently losing provenance.
    #[test]
    fn live_reasoning_node_acquires_copy_unit() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        s.apply_event(&event_seq(
            "assistant/chunk",
            2,
            serde_json::json!({
                "chunk": {"type": "reasoning-delta", "text": "step one "}
            }),
        ));
        s.apply_event(&event_seq(
            "assistant/chunk",
            3,
            serde_json::json!({
                "chunk": {"type": "reasoning-delta", "text": "step two"}
            }),
        ));
        let thinking = s
            .transcript
            .nodes()
            .iter()
            .filter_map(|node| match &node.item {
                DisplayItem::Thinking(node) => Some(node),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            thinking.len(),
            1,
            "live reasoning merges into the breathing node"
        );
        assert_eq!(thinking[0].content, "step one step two");
        let unit = thinking[0]
            .unit
            .expect("live reasoning node must acquire a copy unit");
        assert_eq!(
            s.render.units.get(&unit).map(String::as_str),
            Some("step one step two"),
            "the copy unit maps to the accumulated reasoning content"
        );
    }

    /// Only ADJACENT thinking phases collapse into one row (xN). Any
    /// visible activity in between — a tool card, a read, an assistant
    /// message — starts a fresh Thinking row.
    #[test]
    fn only_adjacent_thinking_phases_collapse() {
        let mut s = AppState::default();
        // Two phases with activity in between: two separate rows, x1 each.
        s.apply_event(&event("turn/start", serde_json::json!({})));
        s.apply_event(&event(
            "tool/call",
            serde_json::json!({
                "callId": "c1", "name": "read", "arguments": "{\"file\": \"ui.rs\"}"
            }),
        ));
        s.apply_event(&event(
            "tool/result",
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result", "toolCallId": "c1",
                    "content": [{"type": "text", "text": "ok"}]
                }]}
            }),
        ));
        let thinking: Vec<&Msg> = s
            .msgs
            .iter()
            .filter(|m| matches!(m, Msg::Thinking(_)))
            .collect();
        assert_eq!(
            thinking.len(),
            2,
            "activity splits the phases: {thinking:?}"
        );
        assert!(
            matches!(thinking[0], Msg::Thinking(card) if card.count == 1),
            "first row keeps x1"
        );
        assert!(
            matches!(thinking[1], Msg::Thinking(card) if card.state == ThinkState::Running && card.count == 1),
            "the new phase starts a fresh row"
        );
        // Directly adjacent phases (a turn ends in thinking and the next one
        // begins): the settled row is revived with x2.
        s.apply_event(&event("turn/end", serde_json::json!({})));
        s.apply_event(&event("turn/start", serde_json::json!({})));
        let thinking: Vec<&Msg> = s
            .msgs
            .iter()
            .filter(|m| matches!(m, Msg::Thinking(_)))
            .collect();
        assert_eq!(thinking.len(), 2, "no new row for an adjacent phase");
        assert!(
            matches!(thinking[1], Msg::Thinking(card)
                if card.state == ThinkState::Running && card.count == 2),
            "adjacent phase revives the same row at x2"
        );
    }

    #[test]
    fn prompt_queue_queues_while_running_and_dispenses_when_idle() {
        let mut s = AppState::default();
        // Idle: the prompt goes out immediately, nothing queued.
        assert!(s.enqueue_or_immediate("直接发"), "idle sends immediately");
        assert!(s.interaction.queue.is_empty());
        // Running: prompts queue up.
        s.session.status = AgentStatus::Running;
        assert!(!s.enqueue_or_immediate("排队1"), "running queues");
        assert!(!s.enqueue_or_immediate("排队2"));
        assert_eq!(s.interaction.queue, vec!["排队1", "排队2"]);
        // Still running (or working on a just-dispatched item): hold.
        assert_eq!(s.take_next_queued(), None);
        s.session.status = AgentStatus::Idle;
        s.session.working = true;
        assert_eq!(
            s.take_next_queued(),
            None,
            "a just-dispatched prompt holds the queue"
        );
        s.session.working = false;
        assert_eq!(s.take_next_queued().as_deref(), Some("排队1"));
        assert_eq!(s.take_next_queued().as_deref(), Some("排队2"));
        assert_eq!(s.take_next_queued(), None);
    }

    #[test]
    fn queue_clears_on_session_switch() {
        let mut s = AppState::default();
        s.session.session_id = Some("a".into());
        s.interaction.queue.push("排队".into());
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert!(
            s.interaction.queue.is_empty(),
            "queued prompts stay with their session"
        );
    }

    #[test]
    fn welcome_sets_authoritative_mode_and_old_bridge_falls_back() {
        let mut s = AppState::default();
        s.apply(
            "welcome",
            &serde_json::json!({
                "sessionId": "a", "status": "idle", "mode": "cordis"
            }),
        );
        assert_eq!(s.session.current_mode.as_deref(), Some("cordis"));

        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert_eq!(
            s.session.current_mode.as_deref(),
            Some(s.config.default_mode.as_str()),
            "welcome from an old bridge keeps the configured fallback"
        );
    }

    #[test]
    fn real_welcome_commits_and_clears_the_local_new_conversation() {
        let mut s = AppState::default();
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId":"old","status":"idle","mode":"standard"}),
        );
        s.begin_new_conversation("code");
        s.apply_event(&serde_json::json!({
            "type": "user/message",
            "seq": 1,
            "data": {"content": [{"type":"text","text":"old event"}], "source":{"kind":"user"}}
        }));
        assert!(s.is_new_conversation());
        assert!(
            !s.transcript.is_empty(),
            "old frames remain retained behind draft"
        );
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId":"new","status":"idle","mode":"code"}),
        );
        assert!(!s.is_new_conversation());
        assert_eq!(s.session.session_id.as_deref(), Some("new"));
        assert!(
            s.transcript.is_empty(),
            "normal switch reset commits the draft"
        );
    }

    #[test]
    fn new_conversation_draft_clears_and_holds_the_preview_empty() {
        let mut s = AppState::default();
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId":"old","status":"idle","mode":"standard"}),
        );
        // A deferred file preview leaves the pane with a live target.
        s.apply_event(&event_seq(
            "tool/call",
            1,
            serde_json::json!({
                "callId": "view-1",
                "name": "str_replace_editor",
                "arguments": "{\"command\":\"view\",\"path\":\"src/main.rs\"}"
            }),
        ));
        assert!(s.preview.target.is_some());
        s.begin_new_conversation("code");
        assert!(s.is_new_conversation());
        assert_eq!(
            s.preview.state,
            e_tui::preview::PreviewState::Empty,
            "the /new page must not inherit the old session's preview"
        );
        assert!(s.preview.target.is_none());
        // Old-session frames keep reducing behind the draft but must not
        // repopulate the preview.
        s.apply_event(&event_seq(
            "assistant/chunk",
            2,
            serde_json::json!({
                "chunk": {"type": "text-delta", "index": 0, "text": "still old"}
            }),
        ));
        assert!(
            s.preview.target.is_none()
                && matches!(s.preview.state, e_tui::preview::PreviewState::Empty),
            "frames behind the draft must not repopulate the preview"
        );
    }

    #[test]
    fn welcome_sets_and_switch_clears_the_title() {
        let mut s = AppState::default();
        s.apply(
            "welcome",
            &serde_json::json!({
                "sessionId": "a", "status": "idle", "title": "第一个标题"
            }),
        );
        assert_eq!(s.session.session_title.as_deref(), Some("第一个标题"));
        // Switching sessions clears the old title; the new welcome's title
        // (or absence of one) replaces it.
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert_eq!(
            s.session.session_title, None,
            "title follows the session switch"
        );
    }

    #[test]
    fn welcome_sets_cwd_and_switch_clears_it() {
        let mut s = AppState::default();
        s.apply(
            "welcome",
            &serde_json::json!({
                "sessionId": "a", "status": "idle", "cwd": r"D:\MyProjects\Chore\dsh"
            }),
        );
        assert_eq!(
            s.session.session_cwd.as_deref(),
            Some(r"D:\MyProjects\Chore\dsh")
        );
        // Switching sessions clears the old path; the new welcome's cwd
        // (or absence of one) replaces it.
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert_eq!(
            s.session.session_cwd, None,
            "cwd follows the session switch"
        );
    }

    #[test]
    fn preset_and_usage_update_status_metadata() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "agent-preset/selected",
            serde_json::json!({ "agentPreset": "cordis" }),
        ));
        s.apply_event(&event(
            "assistant/message",
            serde_json::json!({
                "turn": 1,
                "step": 1,
                "message": { "content": [{ "type": "text", "text": "ok" }] },
                "usage": {
                    "inputTokens": 20,
                    "outputTokens": 4,
                    "cacheReadTokens": 80,
                    "cacheWriteTokens": 0
                }
            }),
        ));
        assert_eq!(s.session.current_mode.as_deref(), Some("cordis"));
        assert_eq!(s.cache_hit_rate(), Some(80));
    }

    #[test]
    fn history_prepend_preserves_newest_mode_and_usage_replacement_anchor() {
        let mut s = AppState::default();
        s.apply_event(&serde_json::json!({
            "type": "agent-preset/selected",
            "seq": 100,
            "data": { "agentPreset": "cordis" }
        }));
        s.apply_event(&serde_json::json!({
            "type": "assistant/chunk",
            "seq": 101,
            "data": {
                "turn": 2,
                "step": 1,
                "chunk": { "type": "usage", "usage": {
                    "inputTokens": 20,
                    "outputTokens": 0,
                    "cacheReadTokens": 80,
                    "cacheWriteTokens": 0
                }}
            }
        }));
        let normalize =
            |value| crate::bridge::adapter::normalize_host_event(HostEvent::from_value(value));
        let older = vec![
            normalize(serde_json::json!({
                "type": "agent-preset/selected",
                "seq": 1,
                "data": { "agentPreset": "standard" }
            })),
            normalize(serde_json::json!({
                "type": "assistant/message",
                "seq": 2,
                "data": {
                    "turn": 1,
                    "step": 1,
                    "message": { "content": [] },
                    "usage": {
                        "inputTokens": 10,
                        "outputTokens": 0,
                        "cacheReadTokens": 0,
                        "cacheWriteTokens": 0
                    }
                }
            })),
        ];
        s.prepend_host_events(&older);
        assert_eq!(s.session.current_mode.as_deref(), Some("cordis"));
        assert_eq!(s.session.token_usage.input_tokens, 30);
        assert_eq!(s.session.token_usage.cache_read_tokens, 80);

        // The final report for the newest request replaces its usage chunk;
        // it must not be added a second time after older history was loaded.
        s.apply_event(&serde_json::json!({
            "type": "assistant/message",
            "seq": 102,
            "data": {
                "turn": 2,
                "step": 1,
                "message": { "content": [] },
                "usage": {
                    "inputTokens": 30,
                    "outputTokens": 0,
                    "cacheReadTokens": 70,
                    "cacheWriteTokens": 0
                }
            }
        }));
        assert_eq!(s.session.token_usage.input_tokens, 40);
        assert_eq!(s.session.token_usage.cache_read_tokens, 70);
    }

    #[test]
    fn title_event_updates_the_title_row() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "session/title",
            serde_json::json!({ "title": "自动生成" }),
        ));
        assert_eq!(s.session.session_title.as_deref(), Some("自动生成"));
        // The title is not part of the transcript cache.
        assert!(s.msgs.is_empty());
    }

    #[test]
    fn snapshot_replay_suppresses_thinking_rows() {
        let mut s = AppState::default();
        s.apply(
            "snapshot",
            &serde_json::json!({
                "events": [
                    event("turn/start", serde_json::json!({})),
                    event("tool/call", serde_json::json!({
                        "callId": "c1", "name": "bash", "arguments": "{}"
                    })),
                    event("tool/result", serde_json::json!({
                        "message": {"content": [{
                            "type": "tool-result", "toolCallId": "c1",
                            "content": [{"type": "text", "text": "ok"}]
                        }]}
                    })),
                    event("turn/end", serde_json::json!({})),
                ],
                "truncated": false
            }),
        );
        assert!(
            !s.msgs.iter().any(|m| matches!(m, Msg::Thinking(_))),
            "history replay has no Thinking rows"
        );
        assert!(!s.session.working, "replayed turn ended");
    }

    /// Replaying a multi-turn conversation must keep each turn's reasoning
    /// in its own Thinking node: without turn identity the second turn's
    /// (non-streaming) reasoning would replace — or merge into — the first
    /// turn's node, losing per-turn content.
    #[test]
    fn snapshot_replay_keeps_per_turn_reasoning_nodes() {
        let mut s = AppState::default();
        s.apply(
            "snapshot",
            &serde_json::json!({
                "events": [
                    event_seq("assistant/message", 1, serde_json::json!({
                        "turn": 1, "step": 1,
                        "message": {"content": [
                            {"type": "reasoning", "text": "turn one reasoning"},
                            {"type": "text", "text": "turn one answer"}
                        ]}
                    })),
                    event_seq("user/message", 2, serde_json::json!({
                        "content": [{"type": "text", "text": "again"}],
                        "source": {"kind": "user"}
                    })),
                    event_seq("assistant/message", 3, serde_json::json!({
                        "turn": 2, "step": 1,
                        "message": {"content": [
                            {"type": "reasoning", "text": "turn two reasoning"},
                            {"type": "text", "text": "turn two answer"}
                        ]}
                    })),
                ],
                "truncated": false
            }),
        );
        let thinking = s
            .transcript
            .nodes()
            .iter()
            .filter_map(|node| match &node.item {
                DisplayItem::Thinking(node) => Some(node),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            thinking.len(),
            2,
            "each replayed turn keeps its own Thinking node"
        );
        assert_eq!(thinking[0].content, "turn one reasoning");
        assert_eq!(thinking[0].turn, Some(1));
        assert!(
            thinking[0].unit.is_some(),
            "replayed reasoning carries a copy unit"
        );
        assert_eq!(thinking[1].content, "turn two reasoning");
        assert_eq!(thinking[1].turn, Some(2));
        assert!(
            thinking[1].unit.is_some(),
            "later-turn reasoning carries its own copy unit"
        );
    }

    #[test]
    fn snapshot_mid_turn_attaches_live_thinking() {
        let mut s = AppState::default();
        s.apply(
            "snapshot",
            &serde_json::json!({
                "events": [
                    event("turn/start", serde_json::json!({})),
                    event("tool/call", serde_json::json!({
                        "callId": "c1", "name": "bash", "arguments": "{}"
                    })),
                    event("tool/result", serde_json::json!({
                        "message": {"content": [{
                            "type": "tool-result", "toolCallId": "c1",
                            "content": [{"type": "text", "text": "ok"}]
                        }]}
                    })),
                ],
                "truncated": false
            }),
        );
        assert!(s.session.working, "tail ended inside a thinking phase");
        assert!(
            matches!(s.msgs.last(), Some(Msg::Thinking(card)) if card.state == ThinkState::Running),
            "mid-turn attach shows one live Thinking row"
        );
    }

    #[test]
    fn snapshot_filters_non_surface() {
        let mut s = AppState::default();
        s.apply("snapshot", &serde_json::json!({
            "events": [
                event("assistant/chunk", serde_json::json!({"chunk": {"type": "text-delta", "text": "x"}})),
                event("user/message", serde_json::json!({
                    "content": [{"type": "text", "text": "hi"}],
                    "source": {"kind": "user"}
                })),
            ]
        }));
        assert_eq!(s.msgs.len(), 1);
        assert!(matches!(&s.msgs[0], Msg::User { .. }));
    }

    /// Perf regression: cache invalidation must be incremental — high-
    /// frequency non-rendered events leave the cache alone and streaming
    /// chunks only dirty the tail, so the render loop never rebuilds the
    /// whole transcript per chunk.
    #[test]
    fn snapshot_replays_unknown_surface_events_but_not_unknown_audit() {
        let mut s = AppState::default();
        s.apply(
            "snapshot",
            &serde_json::json!({
                "events": [
                    {"seq":1,"type":"future/audit","data":{}},
                    {"seq":2,"type":"future/surface","surfaceOp":"append","data":{}},
                    {"seq":3,"type":"future/replace","surfaceOp":{"op":"replace"},"data":{}}
                ]
            }),
        );
        assert!(s.msgs.iter().any(
            |msg| matches!(msg, Msg::Block(block) if block.content.contains("future/surface"))
        ));
        assert!(s
            .msgs
            .iter()
            .any(|msg| matches!(msg, Msg::Error { text } if text.contains("surface operation"))));
        assert!(!s
            .msgs
            .iter()
            .any(|msg| matches!(msg, Msg::Block(block) if block.content.contains("future/audit"))));
        assert!(s.transcript.nodes().iter().any(|node| matches!(
            &node.item,
            DisplayItem::Block(block)
                if block.format == TranscriptFormat::UnknownFallback
                    && block.content.contains("future/surface")
        )));
        assert!(s.transcript.nodes().iter().any(|node| matches!(
            &node.item,
            DisplayItem::Block(block)
                if block.tone == DisplayTone::Error
                    && block.content.contains("surface operation")
        )));
    }

    #[test]
    fn cache_invalidation_is_incremental() {
        let mut s = AppState::default();
        s.render.transcript_cache.valid = true;
        s.render.transcript_cache.tail_dirty = false;
        // Non-rendered events don't touch the cache.
        s.apply_event(&event("step/tool", serde_json::json!({"x": 1})));
        assert!(
            s.render.transcript_cache.valid,
            "step events must not invalidate"
        );
        // Structural change invalidates.
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }),
        ));
        assert!(!s.render.transcript_cache.valid);
        s.render.transcript_cache.valid = true;
        s.render.transcript_cache.tail_dirty = false;
        // The first chunk creates a Streaming message → structural.
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "a"}
            }),
        ));
        assert!(!s.render.transcript_cache.valid);
        s.render.transcript_cache.valid = true;
        s.render.transcript_cache.tail_dirty = false;
        // Appended chunks only dirty the tail.
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "b"}
            }),
        ));
        assert!(
            s.render.transcript_cache.valid,
            "chunk append must not invalidate the cache"
        );
        assert!(
            s.render.transcript_cache.tail_dirty,
            "chunk append must mark the tail dirty"
        );
        assert!(matches!(&s.msgs[1], Msg::Streaming { text } if text == "ab"));
    }

    #[test]
    fn lifecycle_indexes_update_long_command_history_without_duplicates() {
        let mut s = AppState::default();
        for index in 0..2_000u64 {
            s.apply_event(&serde_json::json!({
                "seq":index * 2,"time":index,"type":"command/run",
                "data":{"commandId":format!("c{index}"),"name":"feedback","source":{"kind":"user"}}
            }));
        }
        for index in (0..2_000u64).rev() {
            s.apply_event(&serde_json::json!({
                "seq":index * 2 + 1,"time":index + 1,"type":"command/done",
                "data":{"commandId":format!("c{index}"),"kind":"success"}
            }));
        }
        assert_eq!(
            s.msgs
                .iter()
                .filter(|msg| matches!(msg, Msg::Activity(_)))
                .count(),
            2_000
        );
        assert!(s
            .msgs
            .iter()
            .all(|msg| !matches!(msg, Msg::Activity(row) if row.state.is_active())));
    }

    #[test]
    fn extended_event_fixture_projects_correlated_activities_and_ignores_audit() {
        let values: Vec<Value> = serde_json::from_str(include_str!(
            "../../../bridge/test/fixtures/session-events.json"
        ))
        .unwrap();
        let mut s = AppState::default();
        for value in &values {
            s.apply_event(value);
        }
        let activity = |id: &str| {
            s.msgs.iter().find_map(|msg| match msg {
                Msg::Activity(row) if row.id.0 == id => Some(row),
                _ => None,
            })
        };
        assert_eq!(
            activity("command:cmd-1").unwrap().state,
            ActivityState::Success
        );
        assert_eq!(
            activity("retry:retry-1").unwrap().state,
            ActivityState::Running
        );
        assert_eq!(
            activity("code-dispatch:root-1:code:1").unwrap().state,
            ActivityState::Success
        );
        assert_eq!(
            activity("workflow:run-1").unwrap().state,
            ActivityState::Success
        );
        assert_eq!(
            activity("workflow-agent:run-1:1").unwrap().state,
            ActivityState::Success
        );
        assert_eq!(
            activity("code-dispatch:root-1:code:1").unwrap().parent_id,
            Some(DisplayId::correlated("tool-call", "root-1")),
        );
        assert_eq!(s.todos, vec![("ship".into(), "pending".into())]);
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Card(card) if card.header.as_deref() == Some("Compaction summary"))));
        assert!(!s.msgs.iter().any(|msg| matches!(msg, Msg::System { text } if text.contains("approval/asked") || text.contains("request/header"))));
        let before = s.msgs.len();
        s.apply_command_result("cmd-1", "success", Some("done"));
        assert_eq!(
            s.msgs.len(),
            before,
            "direct command result is deduplicated"
        );
    }

    #[test]
    fn direct_command_result_without_log_activity_uses_public_block() {
        let mut state = AppState::default();
        state.apply_command_result("missing", "error", Some("denied"));
        assert!(matches!(
            &state.transcript.nodes()[0].item,
            DisplayItem::Block(block)
                if block.tone == DisplayTone::Error && block.content == "denied"
        ));
    }

    #[test]
    fn workflow_cancellation_remains_distinct_from_failure() {
        let mut s = AppState::default();
        for event in [
            serde_json::json!({"seq":1,"time":1,"type":"tool-workflow/run-start","data":{"runId":"cancel-run","name":"flow"}}),
            serde_json::json!({"seq":2,"time":2,"type":"tool-workflow/agent-start","data":{"runId":"cancel-run","seq":1,"label":"worker"}}),
            serde_json::json!({"seq":3,"time":3,"type":"tool-workflow/agent-end","data":{"runId":"cancel-run","seq":1,"outcome":"cancelled"}}),
            serde_json::json!({"seq":4,"time":4,"type":"tool-workflow/run-end","data":{"runId":"cancel-run","stopReason":"cancelled"}}),
        ] {
            s.apply_event(&event);
        }
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Activity(row) if row.id == DisplayId::correlated("workflow", "cancel-run") && row.state == ActivityState::Cancelled)));
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Activity(row) if row.id == DisplayId::correlated("workflow-agent", "cancel-run:1") && row.state == ActivityState::Cancelled)));
    }

    #[test]
    fn todo_goal_plan_are_accessory_state_and_turn_retires_todo() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "todo/write",
            serde_json::json!({"todos":[{"content":"one","status":"pending"}]}),
        ));
        s.apply_event(&event("goal/change", serde_json::json!({"summary":"ship"})));
        s.apply_event(&event("plan/mode", serde_json::json!({"mode":"on"})));
        assert_eq!(s.todos.len(), 1);
        assert_eq!(s.goal.as_deref(), Some("ship"));
        assert_eq!(s.plan_mode.as_deref(), Some("on"));
        s.apply_event(&event("turn/start", serde_json::json!({"turn":1})));
        assert!(s.todos.is_empty());
    }

    #[test]
    fn reasoning_context_attachment_and_rich_outcomes_project() {
        let mut s = AppState::default();
        s.apply_event(&serde_json::json!({
            "seq":1,"time":1,"type":"assistant/chunk",
            "data":{"turn":1,"step":1,"chunk":{"type":"reasoning-delta","text":"partial"}}
        }));
        s.apply_event(&serde_json::json!({
            "seq":2,"time":2,"type":"assistant/message","surfaceOp":"append",
            "data":{"turn":1,"step":1,"message":{"content":[
                {"type":"reasoning","text":"final reasoning"},
                {"type":"text","text":"answer"}
            ]}}
        }));
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Thinking(card)
            if card.content == "final reasoning")));
        assert!(s
            .msgs
            .iter()
            .any(|msg| matches!(msg, Msg::Assistant { text, .. } if text == "answer")));

        s.apply_event(&serde_json::json!({
            "seq":3,"time":3,"type":"user/message","surfaceOp":"append",
            "data":{"content":[{"type":"text","text":"rules"},{"type":"image","attachment":{"name":"shot.png"}}],
                    "source":{"kind":"plugin","plugin":"ctx","form":"instructions"}}
        }));
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Card(card) if card.role == CardRole::Context && card.content.contains("shot.png"))));

        for (seq, reason) in [(4, "interrupted"), (5, "max-tokens")] {
            s.apply_event(&serde_json::json!({
                "seq":seq,"time":seq,"type":"turn/end","data":{"reason":{"kind":reason}}
            }));
        }
        assert!(s
            .msgs
            .iter()
            .any(|msg| matches!(msg, Msg::System { text } if text.contains("异常中断"))));
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Block(block) if block.tone == DisplayTone::Warning && block.content.contains("token"))));
    }

    #[test]
    fn tool_uses_top_level_time_error_block_and_trim_qualification() {
        let mut s = AppState::default();
        s.apply_event(&serde_json::json!({
            "seq":1,"time":100,"type":"tool/call","data":{"callId":"c1","name":"bash","arguments":"{}"}
        }));
        s.apply_event(&serde_json::json!({
            "seq":2,"time":190,"type":"tool/result","surfaceOp":"append","data":{
                "dshTuiTrimmed":true,"dshTuiOutputTrimmed":true,
                "message":{"content":[{"type":"tool-result","toolCallId":"c1","isError":true,"content":[{"type":"text","text":"tail"}]}]}
            }
        }));
        assert!(
            matches!(&s.msgs[0], Msg::Tool(card) if card.state == ToolState::Done {
                ok: false, lines: 1, lines_truncated: true, duration_ms: 90
            })
        );
    }

    #[test]
    fn exit_marker_parsing() {
        assert_eq!(exit_marker("ok\n[exit code: 0]"), 0);
        assert_eq!(exit_marker("fail\n[exit code: 7]"), 7);
        assert_eq!(exit_marker("no marker"), 0);
    }

    #[test]
    fn breathing_and_settle_colors() {
        let theme = Theme::ferra();
        assert_eq!(breathing_color(&theme, 0.0), theme.dim, "gray at phase 0");
        assert_eq!(
            breathing_color(&theme, 0.5),
            theme.running,
            "yellow at phase 0.5"
        );
        assert_eq!(breathing_color(&theme, 1.0), theme.dim, "gray at phase 1");
        // Settle transition: starts at the captured color, ends at the target.
        assert_eq!(
            settle_color(theme.dim, theme.ok, std::time::Duration::ZERO),
            theme.dim
        );
        assert_eq!(
            settle_color(
                theme.dim,
                theme.ok,
                std::time::Duration::from_millis(SETTLE_TRANSITION_MS as u64 + 1)
            ),
            theme.ok
        );
    }

    #[test]
    fn tick_drives_breathing_and_transition() {
        let mut s = AppState::default();
        let mut card = ToolCard {
            call_id: "c".into(),
            name: "bash".into(),
            summary: "s".into(),
            state: ToolState::Running,
            frame: 0,
            start_ms: 0,
            done_since: None,
            done_from: None,
        };
        s.msgs.push(Msg::Tool(card.clone()));
        s.render.transcript_cache.valid = true;
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "running drives redraws"
        );
        assert!(
            s.session.activity_epoch.is_some(),
            "epoch set while running"
        );
        assert!(
            s.render.transcript_cache.valid,
            "animation keeps the base cache"
        );
        assert!(
            s.render.transcript_cache.dirty_messages.contains(&0),
            "only the running message is dirty"
        );
        // Settle: the transition still drives redraws.
        card.state = ToolState::Done {
            ok: true,
            lines: 0,
            lines_truncated: false,
            duration_ms: 0,
        };
        card.done_since = Some(std::time::Instant::now());
        card.done_from = Some(Theme::ferra().dim);
        s.msgs[0] = Msg::Tool(card.clone());
        s.render.transcript_cache.valid = true;
        s.render.transcript_cache.dirty_messages.clear();
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "transition animates"
        );
        assert!(s.render.transcript_cache.dirty_messages.contains(&0));
        // An expired transition gets one exact target-color patch, then the
        // clock stops without an extra animation deadline.
        card.done_since = Some(
            std::time::Instant::now()
                - std::time::Duration::from_millis(SETTLE_TRANSITION_MS as u64 + 10),
        );
        s.msgs[0] = Msg::Tool(card);
        s.render.transcript_cache.valid = true;
        s.render.transcript_cache.dirty_messages.clear();
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "expired transition submits its final patch"
        );
        assert!(s.render.transcript_cache.dirty_messages.contains(&0));
        assert!(matches!(
            &s.msgs[0],
            Msg::Tool(card) if card.done_since.is_some() && card.done_from.is_none()
        ));
        s.render.transcript_cache.dirty_messages.clear();
        assert!(!tick_spinners(&mut s, std::time::Instant::now()));
        assert!(s.render.transcript_cache.valid);
        assert!(
            s.session.activity_epoch.is_none(),
            "epoch cleared when idle"
        );
    }

    #[test]
    fn final_file_group_patch_does_not_restart_on_duplicate_result() {
        let completed_at = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let mut state = AppState::default();
        state.msgs.push(Msg::FileGroup(FileGroup {
            items: vec![FileItem {
                action: FileAction::Read,
                call_id: "r1".into(),
                file: "a.rs".into(),
                ok: Some(true),
            }],
            frame: 0,
            done_since: Some(completed_at),
            done_from: Some(Theme::ferra().dim),
        }));
        state.render.transcript_cache.valid = true;
        assert!(tick_spinners(&mut state, std::time::Instant::now()));
        let Msg::FileGroup(group) = &mut state.msgs[0] else {
            unreachable!()
        };
        assert_eq!(group.done_since, Some(completed_at));
        assert!(group.done_from.is_none());
        settle_group(group, Theme::ferra().dim);
        assert!(
            group.done_from.is_none(),
            "duplicate result must not restart settle"
        );
    }

    /// Running status alone (no tool/thinking/streaming, `working` false)
    /// must keep the breathing clock alive — the status-bar bullet breathes
    /// for the whole task, not only while a card is visible.
    #[test]
    fn running_status_drives_breathing_without_visible_activity() {
        let mut s = AppState::default();
        s.session.status = AgentStatus::Running;
        s.session.working = false;
        s.render.transcript_cache.valid = true;
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "running drives redraws"
        );
        assert!(
            s.session.activity_epoch.is_some(),
            "epoch set while running"
        );
        assert!(
            s.render.transcript_cache.valid,
            "status-only animation does not dirty transcript rows"
        );
        assert!(s.render.transcript_cache.dirty_messages.is_empty());
        // Idle with nothing animating stops the clock again.
        s.session.status = AgentStatus::Idle;
        assert!(
            !tick_spinners(&mut s, std::time::Instant::now()),
            "idle stops redraws"
        );
        assert!(
            s.session.activity_epoch.is_none(),
            "epoch cleared when idle"
        );
    }

    #[test]
    fn snapshot_sets_history_window() {
        let mut s = AppState::default();
        s.apply(
            "snapshot",
            &serde_json::json!({
                "events": [
                    event_seq("user/message", 5, serde_json::json!({
                        "content": [{"type": "text", "text": "hi"}],
                        "source": {"kind": "user"}
                    })),
                    event_seq("assistant/message", 9, serde_json::json!({
                        "message": {"content": [{"type": "text", "text": "yo"}]}
                    })),
                ],
                "truncated": true
            }),
        );
        assert_eq!(s.session.min_seq, Some(5), "earliest seq tracked");
        assert!(
            !s.session.history_exhausted,
            "truncated snapshot has older history"
        );
        assert!(!s.session.history_loading);

        let mut s2 = AppState::default();
        s2.apply(
            "snapshot",
            &serde_json::json!({
                "events": [event_seq("user/message", 1, serde_json::json!({
                    "content": [{"type": "text", "text": "hi"}],
                    "source": {"kind": "user"}
                }))],
                "truncated": false
            }),
        );
        assert!(
            s2.session.history_exhausted,
            "full snapshot = nothing older to load"
        );
    }

    #[test]
    fn surface_replace_removes_shadowed_messages_and_inserts_summary() {
        let mut s = AppState::default();
        s.apply_event(&serde_json::json!({
            "seq": 2, "time": 10, "type": "user/message", "surfaceOp": "append",
            "data": {"content":[{"type":"text","text":"old user"}],"source":{"kind":"user"}}
        }));
        s.apply_event(&serde_json::json!({
            "seq": 3, "time": 20, "type": "assistant/message", "surfaceOp": "append",
            "data": {"message":{"content":[{"type":"text","text":"old assistant"}]}}
        }));
        s.apply_event(&serde_json::json!({
            "seq": 14, "time": 30, "type": "user/message",
            "surfaceOp": {"op":"replace","start":2,"end":3},
            "sourceEventSeqs": [2,3],
            "data": {"content":[{"type":"text","text":"summary"}],"source":{"kind":"plugin","form":"recall"}}
        }));
        assert_eq!(s.msgs.len(), 1);
        assert!(matches!(&s.msgs[0], Msg::Card(card) if card.content == "summary"));
        assert_eq!(s.transcript.len(), 1);
        assert!(matches!(
            &s.transcript.nodes()[0].item,
            DisplayItem::Card(card) if card.content == "summary"
        ));
        assert_eq!(
            s.transcript.nodes()[0].surface_seq,
            Some(14),
            "replacement owns the original surface position"
        );
        assert!(s.projector.is_shadowed(2));
    }

    #[test]
    fn repeated_compaction_replacement_removes_the_previous_summary_card_only() {
        let mut s = AppState::default();
        for (seq, text) in [(20, "old-a"), (30, "old-b")] {
            s.apply_event(&serde_json::json!({
                "seq":seq,"time":seq,"type":"user/message","surfaceOp":"append",
                "data":{"content":[{"type":"text","text":text}],"source":{"kind":"user"}}
            }));
        }
        s.apply_event(&serde_json::json!({"seq":50,"time":50,"type":"compaction/start","data":{"compactionId":"c1","turn":null}}));
        s.apply_event(&serde_json::json!({"seq":51,"time":51,"type":"compaction/summary","data":{"compactionId":"c1","summary":[{"type":"text","text":"first"}]}}));
        s.apply_event(&serde_json::json!({
            "seq":52,"time":52,"type":"user/message","surfaceOp":{"op":"replace","start":20,"end":30},"sourceEventSeqs":[20,30],
            "data":{"content":[{"type":"text","text":"first"}],"source":{"kind":"plugin","plugin":"compact","compactionId":"c1"}}
        }));
        s.apply_event(&serde_json::json!({"seq":60,"time":60,"type":"compaction/start","data":{"compactionId":"c2","turn":null}}));
        s.apply_event(&serde_json::json!({"seq":61,"time":61,"type":"compaction/summary","data":{"compactionId":"c2","summary":[{"type":"text","text":"second"}]}}));
        s.apply_event(&serde_json::json!({
            "seq":62,"time":62,"type":"user/message","surfaceOp":{"op":"replace","start":52,"end":52},"sourceEventSeqs":[52],
            "data":{"content":[{"type":"text","text":"second"}],"source":{"kind":"plugin","plugin":"compact","compactionId":"c2"}}
        }));

        let summaries: Vec<&ContentCard> = s
            .msgs
            .iter()
            .filter_map(|msg| match msg {
                Msg::Card(card) if card.header.as_deref() == Some("Compaction summary") => {
                    Some(card)
                }
                _ => None,
            })
            .collect();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].content, "second");
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Activity(row) if row.id == DisplayId::correlated("compaction", "c1"))));
    }

    #[test]
    fn prepend_suppresses_events_shadowed_by_loaded_replacement() {
        let mut s = AppState::default();
        s.apply_event(&serde_json::json!({
            "seq": 14, "time": 30, "type": "user/message",
            "surfaceOp": {"op":"replace","start":2,"end":3},
            "sourceEventSeqs": [2,3],
            "data": {"content":[{"type":"text","text":"summary"}],"source":{"kind":"plugin","form":"recall"}}
        }));
        let added = s.prepend_events(&[
            serde_json::json!({
                "seq":2,"time":10,"type":"user/message","surfaceOp":"append",
                "data":{"content":[{"type":"text","text":"old"}],"source":{"kind":"user"}}
            }),
            serde_json::json!({
                "seq":1,"time":1,"type":"user/message","surfaceOp":"append",
                "data":{"content":[{"type":"text","text":"kept"}],"source":{"kind":"user"}}
            }),
        ]);
        assert_eq!(added, 1);
        assert!(matches!(&s.msgs[0], Msg::User { text } if text == "kept"));
        assert!(matches!(&s.msgs[1], Msg::Card(card) if card.content == "summary"));
    }

    #[test]
    fn prepend_reconciles_lifecycle_results_split_across_page_boundary() {
        let mut s = AppState::default();
        s.apply_event(&serde_json::json!({
            "seq":2,"time":25,"type":"tool/result","surfaceOp":"append","data":{
                "message":{"content":[{"type":"tool-result","toolCallId":"t1","isError":false,"content":[{"type":"text","text":"ok"}]}]}
            }
        }));
        s.apply_event(&serde_json::json!({
            "seq":4,"time":40,"type":"command/done","data":{"commandId":"cmd1","kind":"success","text":"done"}
        }));
        s.apply_event(&serde_json::json!({
            "seq":6,"time":60,"type":"tool/code-dispatch","data":{"subCallId":"root:code:1","isError":false}
        }));
        s.apply_event(&serde_json::json!({
            "seq":8,"time":80,"type":"tool-workflow/run-end","data":{"runId":"run1","stopReason":"completed"}
        }));
        assert!(
            !s.msgs
                .iter()
                .any(|msg| matches!(msg, Msg::Tool(_) | Msg::Activity(_))),
            "terminal halves wait for their older starts"
        );

        s.prepend_events(&[
            serde_json::json!({
                "seq":1,"time":10,"type":"tool/call","surfaceOp":"append","data":{"callId":"t1","name":"bash","arguments":"{\"command\":\"echo ok\"}"}
            }),
            serde_json::json!({
                "seq":3,"time":30,"type":"command/run","data":{"commandId":"cmd1","name":"compact"}
            }),
            serde_json::json!({
                "seq":5,"time":50,"type":"tool/code-dispatch-start","data":{"rootCallId":"root","parentCallId":"root","subCallId":"root:code:1","name":"read","arguments":{}}
            }),
            serde_json::json!({
                "seq":7,"time":70,"type":"tool-workflow/run-start","data":{"runId":"run1","name":"flow"}
            }),
        ]);

        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Tool(card) if matches!(card.state, ToolState::Done { ok: true, duration_ms: 15, .. }))));
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Activity(row) if row.id == DisplayId::correlated("command", "cmd1") && row.state == ActivityState::Success && row.summary == "done")));
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Activity(row) if row.id == DisplayId::correlated("code-dispatch", "root:code:1") && row.state == ActivityState::Success)));
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Activity(row) if row.id == DisplayId::correlated("workflow", "run1") && row.state == ActivityState::Success)));

        s.apply_event(&serde_json::json!({
            "seq":9,"time":90,"type":"user/message","surfaceOp":{"op":"replace","start":2,"end":2},"sourceEventSeqs":[2],
            "data":{"content":[{"type":"text","text":"tool summary"}],"source":{"kind":"plugin","form":"recall"}}
        }));
        assert!(!s
            .msgs
            .iter()
            .any(|msg| matches!(msg, Msg::Tool(card) if card.call_id == "t1")));
    }

    #[test]
    fn prepend_retry_schedule_does_not_duplicate_a_newer_started_row() {
        let mut s = AppState::default();
        s.apply_event(&serde_json::json!({
            "seq":2,"time":20,"type":"llm/retry-started","data":{"retryId":"r1","retry":1}
        }));
        s.prepend_events(&[serde_json::json!({
            "seq":1,"time":10,"type":"llm/retry","data":{"retryId":"r1","retry":1,"maxRetries":2,"delayMs":100,"failure":{"message":"busy"}}
        })]);
        let rows: Vec<&ActivityRow> = s
            .msgs
            .iter()
            .filter_map(|msg| match msg {
                Msg::Activity(row) if row.id == DisplayId::correlated("retry", "r1") => Some(row),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, ActivityState::Running);
        assert_eq!(rows[0].summary, "1/2 · 100ms · busy");
        assert_eq!(rows[0].start_ms, Some(10));
    }

    #[test]
    fn prepend_events_stitches_in_front() {
        let mut s = AppState::default();
        s.apply_event(&event_seq(
            "user/message",
            10,
            serde_json::json!({
                "content": [{"type": "text", "text": "后"}],
                "source": {"kind": "user"}
            }),
        ));
        s.render.transcript_cache.valid = true;
        s.render.transcript_cache.lines = vec![ratatui::text::Line::default(); 4];
        let added = s.prepend_events(&[event_seq(
            "user/message",
            2,
            serde_json::json!({
                "content": [{"type": "text", "text": "前"}],
                "source": {"kind": "user"}
            }),
        )]);
        assert_eq!(added, 1);
        assert_eq!(s.msgs.len(), 2);
        assert!(matches!(&s.msgs[0], Msg::User { text } if text == "前"));
        assert!(matches!(&s.msgs[1], Msg::User { text } if text == "后"));
        let transcript_text = s
            .transcript
            .nodes()
            .iter()
            .filter_map(|node| match &node.item {
                DisplayItem::Card(card) if card.role == CardRole::User => {
                    Some(card.content.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(transcript_text, vec!["前", "后"]);
        assert!(
            !s.render.transcript_cache.valid,
            "prepend forces a full rebuild"
        );
        assert_eq!(s.render.transcript_cache.prepend_anchor, Some(4));
        assert_eq!(s.session.min_seq, Some(2));
    }

    #[test]
    fn prepend_drops_dangling_streaming() {
        let mut s = AppState::default();
        s.apply_event(&event_seq(
            "assistant/message",
            10,
            serde_json::json!({
                "message": {"content": [{"type": "text", "text": "全文"}]}
            }),
        ));
        let added = s.prepend_events(&[
            event_seq(
                "user/message",
                2,
                serde_json::json!({
                    "content": [{"type": "text", "text": "问"}],
                    "source": {"kind": "user"}
                }),
            ),
            event_seq(
                "assistant/chunk",
                8,
                serde_json::json!({
                    "chunk": {"type": "text-delta", "text": "部分"}
                }),
            ),
        ]);
        assert_eq!(added, 1, "dangling Streaming dropped before Assistant");
        assert!(matches!(&s.msgs[0], Msg::User { .. }));
        assert!(matches!(&s.msgs[1], Msg::Assistant { .. }));
    }

    fn qitem(id: &str, options: &[&str]) -> e_tui::agent::Question {
        e_tui::agent::Question {
            id: id.into(),
            question: format!("{id}?"),
            header: None,
            options: if options.is_empty() {
                None
            } else {
                Some(
                    options
                        .iter()
                        .map(|label| e_tui::agent::QuestionOption {
                            label: (*label).to_string(),
                            description: None,
                        })
                        .collect(),
                )
            },
            multi_select: false,
        }
    }

    #[test]
    fn question_batch_enter_advances_then_confirms() {
        let mut q = QuestionBatch::new(
            "r1".into(),
            "s1".into(),
            vec![qitem("a", &["一", "二", "三"]), qitem("b", &["甲", "乙"])],
        );
        assert_eq!(q.current, 0);
        q.step(1);
        assert_eq!(q.sel, 1);
        q.step_question(1);
        assert_eq!(q.current, 1);
        q.step(1);
        assert_eq!(q.sel, 1);
        q.step_question(-1);
        assert_eq!(q.current, 0);
        assert_eq!(q.sel, 1, "question navigation restores its selection");
        // Enter advances and restores the answer already chosen on question 2.
        assert!(q.enter().is_none());
        assert_eq!(q.current, 1);
        assert_eq!(q.sel, 1);
        // Enter on the last question confirms with the full answer list.
        let answers = q.enter().expect("last Enter returns the answers");
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0].id, "a");
        assert_eq!(answers[0].selected, vec!["二".to_string()]);
        assert_eq!(answers[1].id, "b");
        assert_eq!(answers[1].selected, vec!["乙".to_string()]);
    }

    #[test]
    fn question_batch_space_selects_single_and_toggles_multi_without_advancing() {
        let mut single = QuestionBatch::new(
            "single".into(),
            "s1".into(),
            vec![qitem("choice", &["A", "B"])],
        );
        single.step(1);
        single.toggle_selection();
        assert_eq!(single.current, 0);
        assert!(single.is_option_selected(1));
        single.step(-1);
        let answers = single.enter().expect("single question submits");
        assert_eq!(answers[0].selected, ["B"]);

        let mut item = qitem("many", &["A", "B", "C"]);
        item.multi_select = true;
        let mut multiple = QuestionBatch::new("multi".into(), "s1".into(), vec![item]);
        multiple.step(1);
        multiple.toggle_selection();
        multiple.step(1);
        multiple.toggle_selection();
        multiple.step(-1);
        multiple.toggle_selection();
        assert_eq!(multiple.current, 0);
        assert!(!multiple.is_option_selected(1));
        assert!(multiple.is_option_selected(2));
        let answers = multiple.enter().expect("multi question submits");
        assert_eq!(answers[0].selected, ["C"]);
    }

    #[test]
    fn question_batch_free_text_draft_becomes_custom() {
        let mut q = QuestionBatch::new(
            "r2".into(),
            "s1".into(),
            vec![qitem("name", &[]), qitem("ok", &["是", "否"])],
        );
        assert!(q.is_free_text());
        // Left/Right never crash a free-text question.
        q.step(1);
        q.step(-1);
        q.push_char('我');
        q.push_char('好');
        q.backspace();
        q.push_char('爱');
        // First Enter records the draft and advances to the options question.
        assert!(q.enter().is_none());
        assert_eq!(q.current, 1);
        assert_eq!(q.draft, "", "draft clears for the next question");
        q.step(1);
        let answers = q.enter().expect("last Enter returns the answers");
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0].id, "name");
        assert!(answers[0].selected.is_empty());
        assert_eq!(answers[0].custom.as_deref(), Some("我爱"));
        assert_eq!(answers[1].selected, vec!["否".to_string()]);
    }

    #[test]
    fn question_batch_empty_draft_sends_no_custom() {
        let mut q = QuestionBatch::new("r3".into(), "s1".into(), vec![qitem("x", &[])]);
        q.push_char(' ');
        let answers = q.enter().expect("answers returned");
        assert_eq!(answers.len(), 1);
        assert!(answers[0].selected.is_empty());
        assert_eq!(
            answers[0].custom, None,
            "whitespace-only draft drops custom"
        );
    }
}
