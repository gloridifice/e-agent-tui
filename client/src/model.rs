//! Session-event projection into renderable message cards (design §3.2/§3.3).
//!
//! The bridge forwards raw session events; this module folds them into a
//! small owned message model. Tool cards are created on `tool/call` and
//! finalized on `tool/result` (D21/D22); assistant text streams in through
//! `assistant/chunk` and is replaced by the assembled `assistant/message`.

use serde_json::Value;

use ratatui::style::Color;

use crate::cache::TranscriptRenderCache;
use crate::config::{Config, Theme};
#[cfg(test)]
use crate::display::ContentCard;
use crate::display::{
    ActivityRow, ActivityState, CardRole, DisplayId, DisplayItem, DisplayTone, TranscriptBlock,
    TranscriptFormat,
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
    TranscriptStore,
};
use crate::protocol::{
    ClientMessage, HostEvent, HostEventKind, HostSurfaceOp, TokenUsage, CLIENT_REPLAY_EVENT_CAP,
};
#[cfg(test)]
use crate::render::RenderLine;
use crate::render::RenderOptions;
use crate::transcript_layout::MarkdownLayoutRegistry;

/// One breathing cycle (gray → yellow → gray) of the running indicator.
pub const BREATH_CYCLE_MS: u128 = 1600;
/// Completion color transition: the running bullet interpolates from the
/// captured breathing color to the settled color over this duration.
pub const SETTLE_TRANSITION_MS: u128 = 500;

/// Linear interpolation between two RGB colors (`t` clamped to [0, 1]).
pub fn lerp_color(from: Color, to: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let l = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
            Color::Rgb(l(r1, r2), l(g1, g2), l(b1, b2))
        }
        _ => to,
    }
}

/// Breathing color of the running indicator: `working_status.idle` at phase
/// 0, `working_status.running` at 0.5, then idle again at 1.
pub fn breathing_color(theme: &Theme, phase: f64) -> Color {
    let t = (1.0 - (phase * std::f64::consts::TAU).cos()) / 2.0;
    lerp_color(
        theme.working_status.idle.fg,
        theme.working_status.running.fg,
        t,
    )
}

/// Interpolate from the captured breathing color to the settled color over
/// `SETTLE_TRANSITION_MS`; returns `to` once the transition completes.
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Idle,
    Running,
}

/// One pending approval card (design §4.4).
#[derive(Debug, Clone)]
pub struct ApprovalCard {
    pub id: String,
    pub tool_name: String,
    pub reason: String,
}

/// One pending user-question batch (ask_user_question), answered one
/// question at a time in the selection bar (design §4.4). `current` is the
/// question being answered; `sel` the highlighted option; `draft` the typed
/// text for a question that offered no options.
#[derive(Debug, Clone)]
pub struct QuestionBatch {
    pub rpc_id: String,
    pub session_id: String,
    pub questions: Vec<crate::protocol::QuestionItem>,
    pub current: usize,
    pub sel: usize,
    pub draft: String,
    pub answered: Vec<crate::protocol::QuestionAnswer>,
}

impl QuestionBatch {
    pub fn new(
        rpc_id: String,
        session_id: String,
        questions: Vec<crate::protocol::QuestionItem>,
    ) -> Self {
        Self {
            rpc_id,
            session_id,
            questions,
            current: 0,
            sel: 0,
            draft: String::new(),
            answered: Vec::new(),
        }
    }

    /// Options of the question being answered (empty = free-text question).
    pub fn current_options(&self) -> &[crate::protocol::QuestionOption] {
        self.questions
            .get(self.current)
            .and_then(|q| q.options.as_deref())
            .unwrap_or(&[])
    }

    pub fn is_free_text(&self) -> bool {
        self.current_options().is_empty()
    }

    /// ←/→: move the highlighted option (no-op for free-text questions).
    pub fn step(&mut self, delta: isize) {
        let n = self.current_options().len();
        if n == 0 {
            return;
        }
        let next = if delta < 0 {
            self.sel.saturating_sub(delta.unsigned_abs())
        } else {
            (self.sel + delta as usize).min(n - 1)
        };
        self.sel = next;
    }

    /// Printable key for the current free-text question.
    pub fn push_char(&mut self, c: char) {
        if self.is_free_text() {
            self.draft.push(c);
        }
    }

    /// Backspace for the current free-text question.
    pub fn backspace(&mut self) {
        if self.is_free_text() {
            self.draft.pop();
        }
    }

    /// Enter: record the current answer (highlighted option, or the typed
    /// draft for free-text questions) and advance to the next question.
    /// Returns the complete answer list when the last question was just
    /// answered — the caller sends it and drops the batch.
    pub fn enter(&mut self) -> Option<Vec<crate::protocol::QuestionAnswer>> {
        let question = &self.questions[self.current];
        let (selected, custom) = if self.is_free_text() {
            let trimmed = self.draft.trim().to_string();
            let custom = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
            (Vec::new(), custom)
        } else {
            (vec![self.current_options()[self.sel].label.clone()], None)
        };
        self.answered.push(crate::protocol::QuestionAnswer {
            id: question.id.clone(),
            selected,
            custom,
        });
        if self.current + 1 >= self.questions.len() {
            return Some(std::mem::take(&mut self.answered));
        }
        self.current += 1;
        self.sel = 0;
        self.draft.clear();
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewConversationDraft {
    pub mode: String,
    /// The first prompt is retained until a real welcome commits the new
    /// session; `Some` means bridge materialization is in flight.
    pub pending_input: Option<String>,
    /// Draft-local notice rendered without mutating the retained transcript.
    pub notice: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivityTransition {
    pub done_since: std::time::Instant,
    pub from: Color,
}

pub struct AppState {
    /// Public display-surface store. During the family-by-family migration,
    /// remaining legacy families are still rendered from `msgs`; migrated
    /// assistant/content nodes are authoritative here.
    pub transcript: TranscriptStore,
    pub(crate) markdown_layout: MarkdownLayoutRegistry,
    pub(crate) activity_transitions: std::collections::HashMap<DisplayId, ActivityTransition>,
    #[cfg(test)]
    pub msgs: Vec<Msg>,
    /// Typed event classifier, surface ordering, and lifecycle correlations.
    pub projector: EventProjector,
    /// Current whole-list todo projection (rendered as an input accessory).
    pub todos: Vec<(String, String)>,
    pub goal: Option<String>,
    pub plan_mode: Option<String>,
    pub session_state_events: std::collections::HashSet<String>,
    pub session_id: Option<String>,
    /// Client-only `/new` presentation layered over the still-attached real
    /// session. The retained transcript continues to receive old-session
    /// frames until a real welcome commits the switch.
    pub new_conversation: Option<NewConversationDraft>,
    pub status: AgentStatus,
    /// Generic DSH/plugin commands submitted by this client that have not yet
    /// returned a direct result/error. They are independently interruptible
    /// even when the agent itself remains idle.
    active_commands: usize,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Active agent-preset mode, updated by `agent-preset/selected` replay.
    pub current_mode: Option<String>,
    /// Sequence that supplied `current_mode`; older history pages cannot
    /// overwrite newer page state.
    current_mode_seq: Option<u64>,
    /// Provider-reported usage totals used by the status-line cache-hit rate.
    pub token_usage: TokenUsage,
    last_usage_sample: Option<(Option<u64>, Option<u64>, TokenUsage)>,
    /// Latest `session/title` of the attached session — rendered in the
    /// title row below the status bar (live-updated by event frames).
    pub session_title: Option<String>,
    /// Workspace path of the attached session (its header cwd) — rendered
    /// right-aligned in the title row below the status bar.
    pub session_cwd: Option<String>,
    /// Snapshot replay is truncated (guard for huge session logs).
    pub snapshot_truncated: bool,
    /// Live configuration (persisted TOML, editable via /settings).
    pub config: Config,
    /// Render-unit id allocator for the source map.
    pub(crate) next_unit: u64,
    next_thinking_id: u64,
    next_local_display_id: u64,
    pending_transcript_insert: Option<usize>,
    /// Stable ids already present in the newer page while an older history
    /// page is replayed. This preserves cross-page half correlation without
    /// a second DisplayId-to-index adapter.
    replay_newer_display_ids: std::collections::HashSet<DisplayId>,
    /// unit id -> raw markdown source (copy mode).
    pub units: std::collections::HashMap<u64, String>,
    /// Units whose collapsed window is expanded (D13).
    pub expanded: std::collections::HashSet<u64>,
    /// Pending approval awaiting a Y/n answer.
    pub approval: Option<ApprovalCard>,
    /// Pending user-question batch (selection bar replaces the input bar).
    pub question: Option<QuestionBatch>,
    /// Prompts typed while the agent runs: queued here and auto-dispatched
    /// one at a time whenever the agent returns to idle.
    pub queue: Vec<String>,
    /// Last fetched session list (`/resume` Input Page data).
    pub sessions: Vec<crate::protocol::SessionInfo>,
    /// Rendering-owned transcript cache. Session/event projection mutates
    /// messages and invalidates this boundary without owning ratatui details.
    pub transcript_cache: TranscriptRenderCache,
    /// Spinner frame for the streaming indicator.
    pub stream_frame: usize,
    /// Earliest event seq among the loaded transcript (history paging base).
    pub min_seq: Option<u64>,
    /// A scroll-back history request is in flight.
    pub history_loading: bool,
    /// No older events exist (the whole log is loaded).
    pub history_exhausted: bool,
    /// The agent is "working": set as soon as the user sends a message (even
    /// before the turn starts) and cleared by visible activity or an idle
    /// status. Drives the Thinking card; the status-bar breathing bullet is
    /// driven by the running status (plus this flag for the pre-turn window).
    pub working: bool,
    /// While true (snapshot replay / history prepend), the working flag
    /// updates but no Thinking rows enter the transcript — history is
    /// reconstructed without per-phase indicators.
    pub replaying: bool,
    /// Start of the current running period — the global clock of the
    /// breathing animation (all running bullets breathe in sync).
    pub activity_epoch: Option<std::time::Instant>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            transcript: TranscriptStore::default(),
            markdown_layout: MarkdownLayoutRegistry::default(),
            activity_transitions: std::collections::HashMap::new(),
            #[cfg(test)]
            msgs: Vec::new(),
            projector: EventProjector::default(),
            todos: Vec::new(),
            goal: None,
            plan_mode: None,
            session_state_events: std::collections::HashSet::new(),
            session_id: None,
            new_conversation: None,
            status: AgentStatus::Idle,
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
            config: Config::default(),
            next_unit: 0,
            next_thinking_id: 0,
            next_local_display_id: 0,
            pending_transcript_insert: None,
            replay_newer_display_ids: std::collections::HashSet::new(),
            units: std::collections::HashMap::new(),
            expanded: std::collections::HashSet::new(),
            approval: None,
            question: None,
            queue: Vec::new(),
            sessions: Vec::new(),
            transcript_cache: TranscriptRenderCache {
                width: 80,
                ..TranscriptRenderCache::default()
            },
            stream_frame: 0,
            min_seq: None,
            history_loading: false,
            history_exhausted: false,
            working: false,
            replaying: false,
            activity_epoch: None,
        }
    }
}

impl AppState {
    pub fn theme(&self) -> crate::config::Theme {
        self.config.theme()
    }

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

    fn record_usage(&mut self, turn: Option<u64>, step: Option<u64>, usage: Option<TokenUsage>) {
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

    /// Breathing phase in [0, 1) from the current activity epoch; 0 while no
    /// activity has started (gray).
    pub fn breath_phase(&self) -> f64 {
        match self.activity_epoch {
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
        self.working = true;
        if self.replaying {
            return;
        }
        let adjacent_id = self.transcript.nodes().last().and_then(|node| {
            matches!(&node.item, DisplayItem::Activity(row) if row.id.0.starts_with("thinking:"))
                .then(|| node.id().clone())
        });
        if let Some(id) = adjacent_id {
            if let Some(node) = self.transcript.get_mut(&id) {
                if let DisplayItem::Activity(row) = &mut node.item {
                    if row.state != ActivityState::Running {
                        row.state = ActivityState::Running;
                        row.count += 1;
                    }
                }
            }
            self.transcript.touch(&id);
        } else {
            let id = DisplayId::correlated("thinking", &self.next_thinking_id.to_string());
            self.next_thinking_id = self.next_thinking_id.wrapping_add(1);
            let mut row = ActivityRow::root(id, "Thinking...");
            row.count = 1;
            self.insert_transcript_item(DisplayItem::Activity(row), None, None);
        }

        #[cfg(test)]
        match self.msgs.last_mut() {
            Some(Msg::Thinking(card)) if card.state == ThinkState::Running => {}
            Some(Msg::Thinking(card)) => {
                card.state = ThinkState::Running;
                card.done_since = None;
                card.done_from = None;
                card.count += 1;
                self.transcript_cache.valid = false;
            }
            _ => {
                self.msgs.push(Msg::Thinking(ThinkingCard {
                    state: ThinkState::Running,
                    count: 1,
                    done_since: None,
                    done_from: None,
                }));
                self.transcript_cache.valid = false;
            }
        }
    }

    fn capture_activity_transition(&mut self, id: &DisplayId) {
        if self.replaying || self.activity_transitions.contains_key(id) {
            return;
        }
        self.activity_transitions.insert(
            id.clone(),
            ActivityTransition {
                done_since: std::time::Instant::now(),
                from: breathing_color(&self.config.theme(), self.breath_phase()),
            },
        );
    }

    /// The thinking phase ended (visible activity took over, the turn
    /// ended, or the agent went idle): settle the running Thinking row green.
    pub fn stop_thinking(&mut self) {
        self.working = false;
        if self.replaying {
            return;
        }
        let running_id = self.transcript.nodes().iter().rev().find_map(|node| {
            matches!(&node.item, DisplayItem::Activity(row)
                if row.id.0.starts_with("thinking:") && row.state == ActivityState::Running)
            .then(|| node.id().clone())
        });
        if let Some(id) = running_id {
            self.capture_activity_transition(&id);
            if let Some(node) = self.transcript.get_mut(&id) {
                if let DisplayItem::Activity(row) = &mut node.item {
                    row.state = ActivityState::Success;
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
                self.transcript_cache.valid = false;
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
        self.transcript_cache.invalidate();
    }

    pub fn begin_command_execution(&mut self) {
        self.active_commands = self.active_commands.saturating_add(1);
    }

    pub fn finish_command_execution(&mut self) {
        self.active_commands = self.active_commands.saturating_sub(1);
    }

    pub fn has_active_command(&self) -> bool {
        self.active_commands > 0
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
        if self.status == AgentStatus::Running {
            self.queue.push(text.to_string());
            false
        } else {
            true
        }
    }

    /// The agent went idle: pop the next queued prompt for auto-dispatch,
    /// one at a time (each dispatch keeps the agent busy until it returns
    /// to idle again).
    pub fn take_next_queued(&mut self) -> Option<String> {
        if self.status == AgentStatus::Idle && !self.working && !self.queue.is_empty() {
            Some(self.queue.remove(0))
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
                        unit_start: self.next_unit,
                    });
                }
            }
        }
        self.working = false;
        self.transcript_cache.valid = false;
    }
}

impl AppState {
    /// Drop the whole transcript (used when attaching to another session).
    pub fn reset_transcript(&mut self) {
        self.transcript.clear();
        self.markdown_layout.clear();
        self.activity_transitions.clear();
        #[cfg(test)]
        self.msgs.clear();
        self.projector = EventProjector::default();
        self.todos.clear();
        self.goal = None;
        self.plan_mode = None;
        self.current_mode = None;
        self.current_mode_seq = None;
        self.token_usage = TokenUsage::default();
        self.last_usage_sample = None;
        self.session_state_events.clear();
        self.units.clear();
        self.expanded.clear();
        self.next_unit = 0;
        self.next_thinking_id = 0;
        self.next_local_display_id = 0;
        self.pending_transcript_insert = None;
        self.replay_newer_display_ids.clear();
        self.snapshot_truncated = false;
        self.transcript_cache.reset();
        self.min_seq = None;
        self.history_loading = false;
        self.history_exhausted = false;
        self.working = false;
        self.active_commands = 0;
        self.activity_epoch = None;
        // The title belongs to the session being left (welcome sets the
        // new one right after the switch).
        self.session_title = None;
        self.session_cwd = None;
        // Queued prompts belong to the session they were typed for.
        self.queue.clear();
    }
}

impl AppState {
    pub fn begin_new_conversation(&mut self, mode: impl Into<String>) {
        self.new_conversation = Some(NewConversationDraft {
            mode: mode.into(),
            pending_input: None,
            notice: None,
        });
    }

    /// Retain the first prompt and return the atomic materialization payload.
    /// A second submission while creation is in flight is rejected by the
    /// controller rather than entering the retained old session's queue.
    pub fn materialize_new_conversation(&mut self, text: String) -> Option<ClientMessage> {
        let draft = self.new_conversation.as_mut()?;
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
        let draft = self.new_conversation.as_mut()?;
        draft.notice = None;
        draft.pending_input.take()
    }

    pub fn set_new_conversation_notice(&mut self, notice: impl Into<String>) {
        if let Some(draft) = self.new_conversation.as_mut() {
            draft.notice = Some(notice.into());
        }
    }

    pub fn is_new_conversation(&self) -> bool {
        self.new_conversation.is_some()
    }

    /// Apply one bridge message payload (welcome / snapshot / event / status).
    pub fn apply(&mut self, kind: &str, data: &Value) {
        match kind {
            "welcome" => {
                let new_id = data
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(String::from);
                if let (Some(prev), Some(next)) = (&self.session_id, &new_id) {
                    if prev != next {
                        self.reset_transcript();
                        // A pending question belongs to the old session.
                        self.question = None;
                    }
                }
                self.session_id = new_id;
                // Only a real bridge welcome can commit/abandon a local draft.
                self.new_conversation = None;
                self.session_title = data.get("title").and_then(Value::as_str).map(String::from);
                self.session_cwd = data.get("cwd").and_then(Value::as_str).map(String::from);
                self.status = if data.get("status").and_then(Value::as_str) == Some("running") {
                    AgentStatus::Running
                } else {
                    AgentStatus::Idle
                };
                // Snapshot replay refines this: activity events clear it,
                // a mid-thought tail keeps it.
                self.working = self.status == AgentStatus::Running;
                self.provider = data
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(String::from);
                self.model = data.get("model").and_then(Value::as_str).map(String::from);
                self.current_mode = data
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or_else(|| Some(self.config.default_mode.clone()));
                self.current_mode_seq = None;
            }
            "snapshot" => {
                let events: Vec<HostEvent> = data
                    .get("events")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned()
                    .map(HostEvent::from_value)
                    .collect();
                let bridge_truncated = data
                    .get("truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.apply_snapshot(&events, bridge_truncated);
            }
            "event" => self.apply_event(data),
            "status" => {
                self.status = if data.get("status").and_then(Value::as_str) == Some("running") {
                    AgentStatus::Running
                } else {
                    AgentStatus::Idle
                };
                if self.status == AgentStatus::Idle {
                    self.stop_thinking();
                }
            }
            _ => {}
        }
    }

    /// Apply a typed replay window. The client replay budget is generated
    /// from the canonical wire contract and protects the local render cache.
    pub fn apply_snapshot(&mut self, events: &[HostEvent], bridge_truncated: bool) {
        let mut surface: Vec<&HostEvent> = events
            .iter()
            .filter(|event| event.is_replay_relevant())
            .collect();
        let truncated = bridge_truncated || surface.len() > CLIENT_REPLAY_EVENT_CAP;
        if truncated {
            surface = surface.split_off(surface.len().saturating_sub(CLIENT_REPLAY_EVENT_CAP));
            self.snapshot_truncated = true;
            self.push_system_message("（历史较长，仅回放最近消息）");
        }
        self.replaying = true;
        for event in surface {
            self.apply_host_event(event);
        }
        self.replaying = false;
        if self.working {
            self.start_thinking();
        }
        self.history_exhausted = !truncated;
        self.history_loading = false;
    }

    /// Compatibility entry for tests and persisted JSON callers. Raw host
    /// values are translated once at the protocol boundary before reduction.
    pub fn apply_event(&mut self, event: &Value) {
        self.apply_host_event(&HostEvent::from_value(event.clone()));
    }

    /// Classify one typed host event, apply semantic surface effects, then
    /// reduce the event into the compatibility message projection.
    pub fn apply_host_event(&mut self, event: &HostEvent) {
        if let Some(seq) = event.seq {
            self.min_seq = Some(self.min_seq.map_or(seq, |m| m.min(seq)));
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
                    if let Some(HostSurfaceOp::Replace { start, end }) = event.surface_op {
                        self.pending_transcript_insert = self.transcript.first_surface_position(
                            &event.source_event_seqs,
                            start,
                            end,
                        );
                        self.transcript
                            .remove_surfaces(&event.source_event_seqs, start, end);
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
                    self.transcript_cache.invalidate();
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
                            "unhandled typed HostEvent family: {:?}",
                            event.kind
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
                                .seq
                                .filter(|_| event.surface_op == Some(HostSurfaceOp::Append));
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
                                event.seq.filter(|_| is_surface_node(&event.kind)),
                                None,
                            );
                            #[cfg(test)]
                            self.msgs.push(Msg::Card(card));
                        }
                        crate::display::DisplayItem::Activity(row) => self.upsert_activity(row),
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
                                event.seq.filter(|_| is_surface_node(&event.kind)),
                                None,
                            );
                            #[cfg(test)]
                            {
                                self.msgs.push(Msg::Activity(activity));
                                self.msgs.push(Msg::Card(detail));
                            }
                        }
                    }
                    self.transcript_cache.invalidate();
                }
                ProjectionEffect::PageState(PageStateEffect::Title(title)) => {
                    self.session_title = title;
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
    }

    fn record_surface_owner(&mut self, event: &HostEvent) {
        let Some(seq) = event.seq.filter(|_| is_surface_node(&event.kind)) else {
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
                matches!(event.surface_op, Some(HostSurfaceOp::Replace { .. })),
            );
        }
    }

    fn allocate_copy_unit(&mut self, source: &str) -> u64 {
        let unit = self.next_unit;
        self.next_unit += 1;
        self.units.insert(unit, source.to_owned());
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
        self.transcript_cache.invalidate();
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
                self.transcript_cache.invalidate();
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
            self.transcript_cache.invalidate();
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
        self.transcript_cache.invalidate();
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
    fn upsert_reasoning(
        &mut self,
        turn: Option<u64>,
        step: Option<u64>,
        text: &str,
        streaming: bool,
    ) {
        if text.is_empty() {
            return;
        }
        let id = DisplayId::correlated(
            "assistant-reasoning",
            &format!("{}:{}", turn.unwrap_or(0), step.unwrap_or(0)),
        );
        let reasoning_visible = self.config.thinking_display_mode().shows_reasoning();
        if let Some(index) = self
            .msgs
            .iter()
            .position(|msg| matches!(msg, Msg::Block(block) if block.id == id))
        {
            let (source, unit) = {
                let Some(Msg::Block(block)) = self.msgs.get_mut(index) else {
                    return;
                };
                if streaming {
                    block.content.push_str(text);
                    block.copy_source.push_str(text);
                } else {
                    block.content = text.to_owned();
                    block.copy_source = text.to_owned();
                    block.streaming = false;
                }
                (block.copy_source.clone(), block.unit)
            };
            if let Some(unit) = unit {
                self.units.insert(unit, source);
            }
            if reasoning_visible && !self.replaying {
                let is_tail = self
                    .msgs
                    .last()
                    .is_some_and(|msg| matches!(msg, Msg::Block(block) if block.id == id));
                if is_tail {
                    self.transcript_cache.mark_tail_dirty();
                } else {
                    self.transcript_cache.invalidate();
                }
            }
            return;
        }
        let unit = self.allocate_copy_unit(text);
        self.msgs.push(Msg::Block(TranscriptBlock {
            id,
            unit: Some(unit),
            content: text.to_owned(),
            format: TranscriptFormat::Reasoning,
            tone: DisplayTone::Dim,
            copy_source: text.to_owned(),
            streaming,
        }));
        if reasoning_visible && !self.replaying {
            // The new block is a structural append: the previous tail was the
            // Thinking row (or an earlier message), so tail splicing would
            // drop it. Rebuild the cache instead.
            self.transcript_cache.invalidate();
        }
    }
    fn reduce_assistant_event(&mut self, event: &HostEvent, mutations: Vec<AssistantMutation>) {
        match &event.kind {
            HostEventKind::UserMessage { .. } => {
                self.projector.tool_family.close_group();
                self.transcript_cache.invalidate();
                if self.status == AgentStatus::Idle {
                    self.stop_thinking();
                }
            }
            HostEventKind::AssistantChunk {
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
            HostEventKind::AssistantMessage {
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
                    self.transcript_cache.invalidate();
                }
                AssistantMutation::UpsertReasoning(block) => {
                    self.upsert_reasoning_store(block.clone());
                    #[cfg(test)]
                    self.upsert_reasoning(
                        match &event.kind {
                            HostEventKind::AssistantChunk { turn, .. }
                            | HostEventKind::AssistantMessage { turn, .. } => *turn,
                            _ => None,
                        },
                        match &event.kind {
                            HostEventKind::AssistantChunk { step, .. }
                            | HostEventKind::AssistantMessage { step, .. } => *step,
                            _ => None,
                        },
                        &block.content,
                        block.streaming,
                    );
                }
                AssistantMutation::UpsertAnswer(block) => {
                    self.upsert_answer_store(event, block);
                }
            }
        }
    }

    fn append_assistant_item(&mut self, event: &HostEvent, mut item: DisplayItem) {
        match &mut item {
            DisplayItem::Block(block) if block.unit.is_none() => {
                block.unit = Some(self.allocate_copy_unit(&block.copy_source));
            }
            DisplayItem::Card(card) if card.unit.is_none() => {
                card.unit = Some(self.allocate_copy_unit(&card.copy_source));
            }
            _ => {}
        }
        let surface_seq = event.seq.filter(|_| is_surface_node(&event.kind));
        let preferred = matches!(&item, DisplayItem::Card(card) if card.role == CardRole::User)
            .then(|| {
                self.transcript.nodes().last().and_then(|node| {
                    matches!(&node.item, DisplayItem::Activity(row)
                        if row.id.0.starts_with("thinking:") && row.state.is_active())
                    .then(|| self.transcript.len().saturating_sub(1))
                })
            })
            .flatten();
        self.insert_transcript_item(item.clone(), surface_seq, preferred);

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
            DisplayItem::Composite { activity, detail } => {
                self.msgs.push(Msg::Activity(activity));
                self.msgs.push(Msg::Card(detail));
            }
        }
    }

    fn upsert_reasoning_store(&mut self, mut incoming: TranscriptBlock) {
        let id = incoming.id.clone();
        let existed = self.transcript.get(&id).is_some();
        let mut existing_unit = None;
        let mut updated_source = None;
        if let Some(node) = self.transcript.get_mut(&id) {
            if let DisplayItem::Block(block) = &mut node.item {
                if incoming.streaming {
                    block.content.push_str(&incoming.content);
                    block.copy_source.push_str(&incoming.copy_source);
                } else {
                    block.content = incoming.content;
                    block.copy_source = incoming.copy_source;
                    block.streaming = false;
                }
                existing_unit = block.unit;
                updated_source = Some(block.copy_source.clone());
            }
        } else {
            if incoming.unit.is_none() {
                incoming.unit = Some(self.allocate_copy_unit(&incoming.copy_source));
            }
            self.insert_transcript_item(DisplayItem::Block(incoming), None, None);
        }
        if let (Some(unit), Some(source)) = (existing_unit, updated_source) {
            self.units.insert(unit, source);
            self.transcript.touch(&id);
        }
        if self.config.thinking_display_mode().shows_reasoning() && !self.replaying {
            let is_tail = self.transcript.position(&id) == self.transcript.len().checked_sub(1);
            if existed && is_tail {
                self.transcript_cache.mark_tail_dirty();
            } else {
                self.transcript_cache.invalidate();
            }
        }
    }

    fn upsert_answer_store(&mut self, event: &HostEvent, incoming: TranscriptBlock) {
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
            let surface_seq = event.seq.filter(|_| is_surface_node(&event.kind));
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
                self.transcript_cache.mark_tail_dirty();
            } else {
                self.transcript_cache.invalidate();
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
            expanded: self.expanded.clone(),
            collapse_rows: self.config.atomic_collapse_rows,
            mermaid_enabled: self.config.mermaid_enabled,
        };
        let lines = self
            .markdown_layout
            .materialize(
                &id,
                &source,
                &theme,
                &mut self.next_unit,
                &options,
                &mut self.units,
            )
            .to_vec();
        #[cfg(test)]
        let unit_start = self
            .markdown_layout
            .unit_start(&id)
            .unwrap_or(self.next_unit);
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
        self.transcript_cache.invalidate();
    }

    fn project_tool_family(&mut self, event: &HostEvent) -> Option<ToolMutation> {
        let now_ms = host_event_time(event);
        match &event.kind {
            HostEventKind::ToolCall { .. } => self.projector.tool_family.project_call(
                event,
                self.session_cwd.as_deref(),
                self.config.read_merge,
                now_ms,
            ),
            HostEventKind::ToolResult { .. } => {
                self.start_thinking();
                self.projector.tool_family.project_result(event, now_ms)
            }
            HostEventKind::UserMessage { .. }
            | HostEventKind::AssistantChunk { .. }
            | HostEventKind::AssistantMessage { .. } => None,
            _ => {
                self.projector.tool_family.close_group();
                None
            }
        }
    }

    fn reduce_tool_family(&mut self, event: &HostEvent, mutation: ToolMutation) {
        let now_ms = host_event_time(event);
        let mut row = match mutation {
            ToolMutation::Upsert(row) => row,
            ToolMutation::MissingResult => {
                if let HostEventKind::ToolResult {
                    call_id,
                    output,
                    is_error,
                    output_truncated,
                } = &event.kind
                {
                    if let Some(seq) = event.seq {
                        self.projector.record_surface_seq(seq);
                    }
                    self.projector.remember_tool_result(
                        call_id.clone(),
                        PendingToolResult {
                            output: output.clone(),
                            is_error: *is_error,
                            output_truncated: *output_truncated,
                            time_ms: now_ms,
                            surface_seq: event.seq,
                        },
                    );
                }
                return;
            }
        };
        match &event.kind {
            HostEventKind::ToolCall { .. } => {
                self.transcript_cache.invalidate();
                self.stop_thinking();
            }
            HostEventKind::ToolResult { .. } => self.start_thinking(),
            _ => {}
        }

        let row_id = row.id.clone();
        if let HostEventKind::ToolCall { call_id, .. } = &event.kind {
            self.projector
                .tool_calls
                .insert(call_id.clone(), row_id.clone());
        }
        let pending_result = match &event.kind {
            HostEventKind::ToolCall { call_id, .. } => self.projector.take_tool_result(call_id),
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
                let lines = pending.output.lines().count();
                row.continuations
                    .push(crate::display::ActivityContinuation {
                        separator: " · ".into(),
                        label: String::new(),
                        summary: if pending.output_truncated {
                            format!("{lines}+ lines")
                        } else {
                            format!("{lines} lines")
                        },
                    });
            }
        }
        let surface_seq = event.seq.filter(|_| is_surface_node(&event.kind));
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
        if let (HostEventKind::ToolCall { call_id, .. }, Some(pending)) =
            (&event.kind, pending_result)
        {
            self.apply_tool_result_to_display(
                call_id,
                &pending.output,
                pending.is_error,
                pending.output_truncated,
                pending.time_ms,
            );
        }

        #[cfg(test)]
        if let HostEventKind::ToolResult {
            call_id,
            output,
            is_error,
            output_truncated,
        } = &event.kind
        {
            self.apply_tool_result_to_display(
                call_id,
                output,
                *is_error,
                *output_truncated,
                now_ms,
            );
        }
    }

    #[cfg(test)]
    fn upsert_tool_legacy_mirror(&mut self, event: &HostEvent, row: &ActivityRow) {
        let HostEventKind::ToolCall { call_id, .. } = &event.kind else {
            return;
        };
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
                    self.transcript_cache.invalidate();
                    return;
                }
            }
        }
        self.msgs.push(msg);
        self.transcript_cache.invalidate();
    }

    fn reduce_activity_families(&mut self, event: &HostEvent) -> bool {
        if let Some(projection) = lifecycle::project(event) {
            self.apply_lifecycle_projection(event, projection);
            return true;
        }

        let retry_id = match &event.kind {
            HostEventKind::LlmRetry { retry_id, .. }
            | HostEventKind::LlmRetryStarted { retry_id, .. } => Some(retry_id.as_str()),
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

        let (parent_id, parent_depth) = match &event.kind {
            HostEventKind::CodeDispatchStart {
                root_call_id,
                parent_call_id,
                ..
            } => {
                let parent_key = if parent_call_id.is_empty() {
                    root_call_id
                } else {
                    parent_call_id
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
                    if matches!(event.kind, HostEventKind::WorkflowRunStart { .. }) {
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
                    self.transcript_cache.invalidate();
                } else {
                    self.projector.remember_activity_enrichment(
                        id,
                        PendingActivityEnrichment { summary, start_ms },
                    );
                }
            }
        }
    }

    fn apply_lifecycle_projection(&mut self, event: &HostEvent, projection: LifecycleProjection) {
        match projection {
            LifecycleProjection::TurnStart => {
                self.start_thinking();
                self.activity_epoch
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
                    self.transcript_cache.invalidate();
                }
            }
            LifecycleProjection::Goal(goal) => self.goal = goal,
            LifecycleProjection::Plan(plan) => self.plan_mode = plan,
            LifecycleProjection::Preset(preset) => {
                let is_newest = match (event.seq, self.current_mode_seq) {
                    (Some(incoming), Some(current)) => incoming >= current,
                    (Some(_), None) | (None, None) => true,
                    (None, Some(_)) => false,
                };
                if is_newest && !preset.is_empty() {
                    self.current_mode = Some(preset);
                    self.current_mode_seq = event.seq;
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
        let typed: Vec<HostEvent> = events.iter().cloned().map(HostEvent::from_value).collect();
        self.prepend_host_events(&typed)
    }

    /// Prepend one typed history page while preserving the current viewport anchor.
    pub fn prepend_host_events(&mut self, events: &[HostEvent]) -> usize {
        let existing_owners = self.projector.owned_seqs();
        // Token totals should absorb older pages, but replacement bookkeeping
        // must keep pointing at the newest loaded request.
        let newest_usage_sample = self.last_usage_sample;
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
            self.last_usage_sample = newest_usage_sample;
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
        self.transcript_cache.invalidate();
        self.transcript_cache.tail_dirty = false;
        self.transcript_cache.prepend_anchor = Some(self.transcript_cache.display_len());
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
            DisplayItem::Card(_) => false,
        })
        || !state.activity_transitions.is_empty()
        || state.working
        || state.status == AgentStatus::Running
}

/// Advance the breathing/transition animation clock and mark only the public
/// display ranges whose colors can change. Expired transitions submit one
/// final exact-color patch before their sidecar entry is removed.
pub fn tick_spinners(state: &mut AppState, now: std::time::Instant) -> bool {
    #[cfg(test)]
    if state.transcript.is_empty() && !state.msgs.is_empty() {
        return tick_legacy_spinners(state, now);
    }

    let mut any_pending = state.working || state.status == AgentStatus::Running;
    let mut dirty = Vec::new();
    for (index, node) in state.transcript.nodes().iter().enumerate() {
        let pending = match &node.item {
            DisplayItem::Activity(row) => row.state.is_active(),
            DisplayItem::Block(block) => {
                block.streaming && block.format != TranscriptFormat::Reasoning
            }
            DisplayItem::Composite { activity, .. } => activity.state.is_active(),
            DisplayItem::Card(_) => false,
        };
        if pending {
            any_pending = true;
            dirty.push(index);
        }
    }

    let transitions = state
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
        state.activity_transitions.remove(&id);
    }

    if any_pending {
        state.activity_epoch.get_or_insert(now);
    } else if state.activity_transitions.is_empty() {
        state.activity_epoch = None;
    }
    dirty.sort_unstable();
    dirty.dedup();
    let animation_changed = !dirty.is_empty();
    for index in dirty {
        state.transcript_cache.mark_message_dirty(index);
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
        || state.working
        || state.status == AgentStatus::Running;
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
    let mut any_pending = state.working || state.status == AgentStatus::Running;
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
        state.activity_epoch.get_or_insert(now);
    } else {
        state.activity_epoch = None;
    }
    for index in dirty {
        state.transcript_cache.mark_message_dirty(index);
    }
    any_pending || animation_changed
}

fn host_event_time(event: &HostEvent) -> u64 {
    event.time_ms.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    })
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
            .markdown_layout
            .unit_start(s.transcript.nodes()[0].id())
            .is_some());
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
        s.session_cwd = Some(r"G:\workspace".into());
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
        assert!(s.working, "Working after turn/start");
        s.apply_event(&event(
            "tool/call",
            serde_json::json!({
                "callId": "c1", "name": "bash", "arguments": "{}"
            }),
        ));
        assert!(!s.working, "tool call is visible activity");
        s.apply_event(&event(
            "tool/result",
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result", "toolCallId": "c1",
                    "content": [{"type": "text", "text": "ok"}]
                }]}
            }),
        ));
        assert!(s.working, "model works again after a tool result");
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "hi"}
            }),
        ));
        assert!(!s.working);
        s.apply_event(&event("turn/end", serde_json::json!({})));
        assert!(!s.working);
        // Idle status clears a stale flag.
        s.working = true;
        s.apply("status", &serde_json::json!({"status": "idle"}));
        assert!(!s.working);
        // The echo of the user's own message keeps Working while the agent
        // runs, and clears it when idle (no work follows the echo).
        s.working = true;
        s.status = AgentStatus::Running;
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }),
        ));
        assert!(s.working, "running echo keeps Working alive");
        s.status = AgentStatus::Idle;
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }),
        ));
        assert!(!s.working, "idle echo clears Working");
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
        assert!(!s.working);
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
        for transition in s.activity_transitions.values_mut() {
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
        s.transcript_cache.dirty_messages.clear();
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
        assert!(!s.working);
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
        assert!(!s.working);
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
        assert!(s.working, "still working while reasoning streams");
        assert!(
            matches!(
                s.msgs.iter().rev().find(|m| matches!(m, Msg::Thinking(_))),
                Some(Msg::Thinking(card)) if card.state == ThinkState::Running
            ),
            "reasoning keeps the Thinking row running"
        );
        assert!(
            s.msgs.iter().any(|m| matches!(m, Msg::Block(block) if block.format == TranscriptFormat::Reasoning && block.content == "thinking out loud")),
            "reasoning block is stored (hidden) rather than dropped"
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
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Block(block)
            if block.format == TranscriptFormat::Reasoning && block.unit.is_some())));
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
        assert!(s.queue.is_empty());
        // Running: prompts queue up.
        s.status = AgentStatus::Running;
        assert!(!s.enqueue_or_immediate("排队1"), "running queues");
        assert!(!s.enqueue_or_immediate("排队2"));
        assert_eq!(s.queue, vec!["排队1", "排队2"]);
        // Still running (or working on a just-dispatched item): hold.
        assert_eq!(s.take_next_queued(), None);
        s.status = AgentStatus::Idle;
        s.working = true;
        assert_eq!(
            s.take_next_queued(),
            None,
            "a just-dispatched prompt holds the queue"
        );
        s.working = false;
        assert_eq!(s.take_next_queued().as_deref(), Some("排队1"));
        assert_eq!(s.take_next_queued().as_deref(), Some("排队2"));
        assert_eq!(s.take_next_queued(), None);
    }

    #[test]
    fn queue_clears_on_session_switch() {
        let mut s = AppState::default();
        s.session_id = Some("a".into());
        s.queue.push("排队".into());
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert!(s.queue.is_empty(), "queued prompts stay with their session");
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
        assert_eq!(s.current_mode.as_deref(), Some("cordis"));

        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert_eq!(
            s.current_mode.as_deref(),
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
        assert_eq!(s.session_id.as_deref(), Some("new"));
        assert!(
            s.transcript.is_empty(),
            "normal switch reset commits the draft"
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
        assert_eq!(s.session_title.as_deref(), Some("第一个标题"));
        // Switching sessions clears the old title; the new welcome's title
        // (or absence of one) replaces it.
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert_eq!(s.session_title, None, "title follows the session switch");
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
        assert_eq!(s.session_cwd.as_deref(), Some(r"D:\MyProjects\Chore\dsh"));
        // Switching sessions clears the old path; the new welcome's cwd
        // (or absence of one) replaces it.
        s.apply(
            "welcome",
            &serde_json::json!({"sessionId": "b", "status": "idle"}),
        );
        assert_eq!(s.session_cwd, None, "cwd follows the session switch");
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
        assert_eq!(s.current_mode.as_deref(), Some("cordis"));
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
        let older = vec![
            HostEvent::from_value(serde_json::json!({
                "type": "agent-preset/selected",
                "seq": 1,
                "data": { "agentPreset": "standard" }
            })),
            HostEvent::from_value(serde_json::json!({
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
        assert_eq!(s.current_mode.as_deref(), Some("cordis"));
        assert_eq!(s.token_usage.input_tokens, 30);
        assert_eq!(s.token_usage.cache_read_tokens, 80);

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
        assert_eq!(s.token_usage.input_tokens, 40);
        assert_eq!(s.token_usage.cache_read_tokens, 70);
    }

    #[test]
    fn title_event_updates_the_title_row() {
        let mut s = AppState::default();
        s.apply_event(&event(
            "session/title",
            serde_json::json!({ "title": "自动生成" }),
        ));
        assert_eq!(s.session_title.as_deref(), Some("自动生成"));
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
        assert!(!s.working, "replayed turn ended");
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
        assert!(s.working, "tail ended inside a thinking phase");
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
        s.transcript_cache.valid = true;
        s.transcript_cache.tail_dirty = false;
        // Non-rendered events don't touch the cache.
        s.apply_event(&event("step/tool", serde_json::json!({"x": 1})));
        assert!(s.transcript_cache.valid, "step events must not invalidate");
        // Structural change invalidates.
        s.apply_event(&event(
            "user/message",
            serde_json::json!({
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }),
        ));
        assert!(!s.transcript_cache.valid);
        s.transcript_cache.valid = true;
        s.transcript_cache.tail_dirty = false;
        // The first chunk creates a Streaming message → structural.
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "a"}
            }),
        ));
        assert!(!s.transcript_cache.valid);
        s.transcript_cache.valid = true;
        s.transcript_cache.tail_dirty = false;
        // Appended chunks only dirty the tail.
        s.apply_event(&event(
            "assistant/chunk",
            serde_json::json!({
                "chunk": {"type": "text-delta", "text": "b"}
            }),
        ));
        assert!(
            s.transcript_cache.valid,
            "chunk append must not invalidate the cache"
        );
        assert!(
            s.transcript_cache.tail_dirty,
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
            "../../bridge/test/fixtures/session-events.json"
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
        assert!(s.msgs.iter().any(|msg| matches!(msg, Msg::Block(block) if block.format == TranscriptFormat::Reasoning && block.content == "final reasoning" && !block.streaming)));
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
        s.transcript_cache.valid = true;
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "running drives redraws"
        );
        assert!(s.activity_epoch.is_some(), "epoch set while running");
        assert!(s.transcript_cache.valid, "animation keeps the base cache");
        assert!(
            s.transcript_cache.dirty_messages.contains(&0),
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
        s.transcript_cache.valid = true;
        s.transcript_cache.dirty_messages.clear();
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "transition animates"
        );
        assert!(s.transcript_cache.dirty_messages.contains(&0));
        // An expired transition gets one exact target-color patch, then the
        // clock stops without an extra animation deadline.
        card.done_since = Some(
            std::time::Instant::now()
                - std::time::Duration::from_millis(SETTLE_TRANSITION_MS as u64 + 10),
        );
        s.msgs[0] = Msg::Tool(card);
        s.transcript_cache.valid = true;
        s.transcript_cache.dirty_messages.clear();
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "expired transition submits its final patch"
        );
        assert!(s.transcript_cache.dirty_messages.contains(&0));
        assert!(matches!(
            &s.msgs[0],
            Msg::Tool(card) if card.done_since.is_some() && card.done_from.is_none()
        ));
        s.transcript_cache.dirty_messages.clear();
        assert!(!tick_spinners(&mut s, std::time::Instant::now()));
        assert!(s.transcript_cache.valid);
        assert!(s.activity_epoch.is_none(), "epoch cleared when idle");
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
        state.transcript_cache.valid = true;
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
        s.status = AgentStatus::Running;
        s.working = false;
        s.transcript_cache.valid = true;
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "running drives redraws"
        );
        assert!(s.activity_epoch.is_some(), "epoch set while running");
        assert!(
            s.transcript_cache.valid,
            "status-only animation does not dirty transcript rows"
        );
        assert!(s.transcript_cache.dirty_messages.is_empty());
        // Idle with nothing animating stops the clock again.
        s.status = AgentStatus::Idle;
        assert!(
            !tick_spinners(&mut s, std::time::Instant::now()),
            "idle stops redraws"
        );
        assert!(s.activity_epoch.is_none(), "epoch cleared when idle");
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
        assert_eq!(s.min_seq, Some(5), "earliest seq tracked");
        assert!(!s.history_exhausted, "truncated snapshot has older history");
        assert!(!s.history_loading);

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
            s2.history_exhausted,
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
        s.transcript_cache.valid = true;
        s.transcript_cache.lines = vec![ratatui::text::Line::default(); 4];
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
        assert!(!s.transcript_cache.valid, "prepend forces a full rebuild");
        assert_eq!(s.transcript_cache.prepend_anchor, Some(4));
        assert_eq!(s.min_seq, Some(2));
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

    fn qitem(id: &str, options: &[&str]) -> crate::protocol::QuestionItem {
        crate::protocol::QuestionItem {
            id: id.into(),
            question: format!("{id}?"),
            header: None,
            options: if options.is_empty() {
                None
            } else {
                Some(
                    options
                        .iter()
                        .map(|label| crate::protocol::QuestionOption {
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
        // Enter selects "二" and moves to the second question.
        assert!(q.enter().is_none());
        assert_eq!(q.current, 1);
        assert_eq!(q.sel, 0, "selection resets for the next question");
        // Step past the end clamps onto the last option.
        q.step(5);
        assert_eq!(q.sel, 1);
        q.step(-1);
        assert_eq!(q.sel, 0);
        // Enter on the last question confirms with the full answer list.
        let answers = q.enter().expect("last Enter returns the answers");
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0].id, "a");
        assert_eq!(answers[0].selected, vec!["二".to_string()]);
        assert_eq!(answers[1].id, "b");
        assert_eq!(answers[1].selected, vec!["甲".to_string()]);
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
