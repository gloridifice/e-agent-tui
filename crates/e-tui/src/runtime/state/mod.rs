//! Provider-neutral runtime state and normalized timeline reduction.
//!
//! Agent adapters provide typed timeline records. This module folds those
//! records into the frontend's display model without retaining provider wire
//! names or transport values.

#[cfg(test)]
use ratatui::style::Color;

const FRONTEND_REPLAY_EVENT_CAP: usize = 2_000;

use crate::AgentRequest;
mod animation;
mod reduction;
mod session;

pub use crate::app::{
    breathing_color, lerp_color, settle_color, BREATH_CYCLE_MS, SETTLE_TRANSITION_MS,
};
use crate::display::{
    ActivityRow, ActivityState, CardRole, DisplayId, DisplayItem, DisplayTone, ThinkingNode,
    TranscriptBlock, TranscriptFormat,
};
use crate::projection::{
    assistant::AssistantMutation, command::CommandProjection, is_surface_node,
    lifecycle::LifecycleProjection, tool::ToolMutation, workflow::WorkflowProjection,
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
            let mut row = ActivityRow::root(
                id,
                crate::i18n::tr(self.config.language, "transcript.thinking"),
            );
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

pub use animation::{animation_active, tick_spinners};

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
                starts_thinking: true,
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

    #[test]
    fn pi_tool_result_waits_for_the_following_turn_start() {
        let mut state = RuntimeState::default();
        state.apply_host_event(&record(1, TimelineFact::ToolCall(edit_call())));
        state.apply_host_event(&record(
            2,
            TimelineFact::ToolResult {
                activity_id: "edit-1".into(),
                output: "ok".into(),
                state: AgentActivityState::Success,
                output_truncated: false,
                starts_thinking: false,
                mutation_diff: None,
                mutation_hunks: Vec::new(),
            },
        ));
        state.apply_host_event(&record(
            3,
            TimelineFact::TurnEnd {
                reason: None,
                error_message: None,
                error_code: None,
            },
        ));
        assert!(state
            .transcript
            .nodes()
            .iter()
            .all(|node| !matches!(node.item, DisplayItem::Thinking(_))));

        state.apply_host_event(&record(4, TimelineFact::TurnStart));
        let thinking = state
            .transcript
            .nodes()
            .iter()
            .find_map(|node| match &node.item {
                DisplayItem::Thinking(node) => Some(node),
                _ => None,
            })
            .expect("turn start creates one Thinking row");
        assert_eq!(thinking.row.count, 1);
        assert_eq!(thinking.row.state, ActivityState::Running);
    }
}
