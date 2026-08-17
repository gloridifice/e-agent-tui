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
use crate::display::{
    ActivityRow, ActivityState, CardRole, ContentCard, DisplayId, DisplayTone, TranscriptBlock,
    TranscriptFormat,
};
use crate::projection::{
    is_surface_node, AccessoryStateEffect, EventProjector, PageStateEffect,
    PendingActivityEnrichment, PendingActivityResult, PendingToolResult, ProjectionEffect,
};
use crate::protocol::{
    HostContentBlock, HostEvent, HostEventKind, HostLifecycleOutcome, HostSurfaceOp, TokenUsage,
    CLIENT_REPLAY_EVENT_CAP,
};
use crate::render::RenderLine;

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

/// Breathing color of the running indicator: gray (`dim`) at phase 0,
/// yellow (`running`) at 0.5, gray again at 1 (one cosine breath per cycle).
pub fn breathing_color(theme: &Theme, phase: f64) -> Color {
    let t = (1.0 - (phase * std::f64::consts::TAU).cos()) / 2.0;
    lerp_color(theme.dim, theme.running, t)
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

/// Command cards (bash/pwsh/…) show command + live output line count (D21).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileAction {
    Read,
    View,
    Edit,
    Replace,
    Insert,
    Create,
}

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
#[derive(Debug, Clone)]
pub struct FileGroup {
    pub items: Vec<FileItem>,
    pub frame: usize,
    /// Settle transition (whole group): captured when the last pending item
    /// settles, animated toward umber/red instead of snapping.
    pub done_since: Option<std::time::Instant>,
    pub done_from: Option<Color>,
}

#[derive(Debug, Clone)]
pub struct FileItem {
    pub action: FileAction,
    pub call_id: String,
    pub file: String,
    /// None = still pending.
    pub ok: Option<bool>,
}

impl FileGroup {
    pub fn pending(&self) -> bool {
        self.items.iter().any(|item| item.ok.is_none())
    }
}

/// Screen rows a file group renders. MUST match `ui::msg_lines` exactly —
/// copy-mode row math (global_row) depends on it. Failed items render one
/// line per DISTINCT action/file pair (repeats collapse into `name xN`).
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
#[derive(Debug, Clone)]
pub enum Msg {
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

/// Lifecycle of the "Thinking..." row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThinkState {
    /// The model is between visible events (or waiting for its turn).
    Running,
    /// Visible activity took over or the turn ended.
    Done,
}

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

pub struct AppState {
    pub msgs: Vec<Msg>,
    /// Typed event classifier, surface ordering, and lifecycle correlations.
    pub projector: EventProjector,
    /// Current whole-list todo projection (rendered as an input accessory).
    pub todos: Vec<(String, String)>,
    pub goal: Option<String>,
    pub plan_mode: Option<String>,
    pub session_state_events: std::collections::HashSet<String>,
    pub session_id: Option<String>,
    pub status: AgentStatus,
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
    /// Last fetched session list (picker data).
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
            msgs: Vec::new(),
            projector: EventProjector::default(),
            todos: Vec::new(),
            goal: None,
            plan_mode: None,
            session_state_events: std::collections::HashSet::new(),
            session_id: None,
            status: AgentStatus::Idle,
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
        match self.msgs.last_mut() {
            Some(Msg::Thinking(card)) if card.state == ThinkState::Running => {}
            Some(Msg::Thinking(card)) => {
                // Directly adjacent phases: revive the settled row and grow
                // its count instead of stacking rows.
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

    /// The thinking phase ended (visible activity took over, the turn
    /// ended, or the agent went idle): settle the running Thinking row green.
    pub fn stop_thinking(&mut self) {
        self.working = false;
        if self.replaying {
            return;
        }
        let breath_now = breathing_color(&self.config.theme(), self.breath_phase());
        if let Some(Msg::Thinking(card)) = self.msgs.last_mut() {
            if card.state == ThinkState::Running {
                card.state = ThinkState::Done;
                card.done_since = Some(std::time::Instant::now());
                card.done_from = Some(breath_now);
                self.transcript_cache.valid = false;
            }
        }
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
                self.msgs.push(Msg::Error {
                    text: text.to_owned(),
                });
            } else {
                self.msgs.push(Msg::System {
                    text: text.to_owned(),
                });
            }
            self.transcript_cache.invalidate();
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
        // A dangling streaming tail becomes assistant source. Presentation
        // materialization is deferred to `presentation.rs`.
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
        self.working = false;
        self.transcript_cache.valid = false;
    }
}

impl AppState {
    /// Drop the whole transcript (used when attaching to another session).
    pub fn reset_transcript(&mut self) {
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
        self.snapshot_truncated = false;
        self.transcript_cache.reset();
        self.min_seq = None;
        self.history_loading = false;
        self.history_exhausted = false;
        self.working = false;
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
            self.msgs.push(Msg::System {
                text: "（历史较长，仅回放最近消息）".into(),
            });
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
        let mut replacement_insert = None;
        for effect in effects {
            match effect {
                ProjectionEffect::SurfaceMutation {
                    mut remove_indices,
                    insert_at,
                } => {
                    remove_indices.sort_unstable_by(|left, right| right.cmp(left));
                    for index in remove_indices {
                        if index < self.msgs.len() {
                            self.msgs.remove(index);
                            self.projector.remove_display_index(index);
                        }
                    }
                    replacement_insert = insert_at;
                    self.transcript_cache.invalidate();
                }
                ProjectionEffect::Reduce { insert_at } => {
                    let before = self.msgs.len();
                    self.reduce_host_event(event);
                    let target = insert_at.or(replacement_insert);
                    if let Some(target) = target.filter(|target| *target < before) {
                        let appended: Vec<Msg> = self.msgs.drain(before..).collect();
                        self.msgs.splice(target..target, appended);
                    }
                    self.record_surface_owner(event, target);
                }
                ProjectionEffect::Display(item) => {
                    match item {
                        crate::display::DisplayItem::Block(mut block) => {
                            if block.unit.is_none() {
                                block.unit = Some(self.allocate_copy_unit(&block.copy_source));
                            }
                            let index = self.msgs.len();
                            self.msgs.push(Msg::Block(block));
                            if let Some(seq) = event
                                .seq
                                .filter(|_| event.surface_op == Some(HostSurfaceOp::Append))
                            {
                                self.projector.record_surface_owner(seq, index, false);
                            }
                        }
                        crate::display::DisplayItem::Card(mut card) => {
                            if card.unit.is_none() {
                                card.unit = Some(self.allocate_copy_unit(&card.copy_source));
                            }
                            self.msgs.push(Msg::Card(card));
                        }
                        crate::display::DisplayItem::Activity(row) => self.upsert_activity(row),
                        crate::display::DisplayItem::Composite {
                            activity,
                            mut detail,
                        } => {
                            self.upsert_activity(activity);
                            if detail.unit.is_none() {
                                detail.unit = Some(self.allocate_copy_unit(&detail.copy_source));
                            }
                            self.msgs.push(Msg::Card(detail));
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
                    self.msgs.push(Msg::Error { text: message });
                    self.transcript_cache.invalidate();
                }
                ProjectionEffect::Ignore => {}
            }
        }
    }

    fn record_surface_owner(&mut self, event: &HostEvent, insertion: Option<usize>) {
        let Some(seq) = event.seq.filter(|_| is_surface_node(&event.kind)) else {
            return;
        };
        let index = match &event.kind {
            HostEventKind::UserMessage { source_kind, .. } => {
                if source_kind.as_deref() == Some("user")
                    && matches!(self.msgs.last(), Some(Msg::Thinking(_)))
                {
                    self.msgs.len().checked_sub(2)
                } else {
                    self.msgs.len().checked_sub(1)
                }
            }
            HostEventKind::AssistantMessage { .. } => self
                .msgs
                .iter()
                .rposition(|msg| matches!(msg, Msg::Assistant { .. })),
            HostEventKind::ToolResult { call_id, .. } => {
                self.msgs.iter().rposition(|msg| match msg {
                    Msg::Tool(card) => card.call_id == *call_id,
                    Msg::FileGroup(group) => {
                        group.items.iter().any(|item| item.call_id == *call_id)
                    }
                    _ => false,
                })
            }
            _ => None,
        };
        if let Some(mut index) = index {
            if let Some(target) = insertion.filter(|target| *target < index) {
                index = target;
            }
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

    fn push_block(&mut self, mut block: TranscriptBlock) {
        if block.unit.is_none() {
            block.unit = Some(self.allocate_copy_unit(&block.copy_source));
        }
        self.msgs.push(Msg::Block(block));
    }

    fn push_card(&mut self, mut card: ContentCard) {
        if card.unit.is_none() {
            card.unit = Some(self.allocate_copy_unit(&card.copy_source));
        }
        self.msgs.push(Msg::Card(card));
    }

    fn upsert_activity(&mut self, mut row: crate::display::ActivityRow) {
        let id = row.id.clone();
        if let Some(pending) = self.projector.take_activity_result(&id) {
            row.state = pending.state;
            if let Some(summary) = pending.summary {
                row.summary = summary;
            }
        }
        if let Some(index) = self.projector.display_position(&id) {
            if let Some(Msg::Activity(existing)) = self.msgs.get_mut(index) {
                *existing = row;
                self.transcript_cache.invalidate();
                return;
            }
        }
        let index = self.msgs.len();
        self.msgs.push(Msg::Activity(row));
        self.projector.record_display_position(id, index);
        self.transcript_cache.invalidate();
    }

    fn apply_pending_activity_enrichments(&mut self) {
        for (id, enrichment) in self.projector.take_activity_enrichments() {
            let Some(index) = self.projector.display_position(&id) else {
                continue;
            };
            if let Some(Msg::Activity(row)) = self.msgs.get_mut(index) {
                row.summary = enrichment.summary;
                if enrichment.start_ms.is_some() {
                    row.start_ms = enrichment.start_ms;
                }
                self.transcript_cache.invalidate();
            }
        }
    }

    fn settle_activity_state(
        &mut self,
        id: &DisplayId,
        state: ActivityState,
        summary: Option<&str>,
    ) -> bool {
        let Some(index) = self.projector.display_position(id) else {
            return false;
        };
        if let Some(Msg::Activity(row)) = self.msgs.get_mut(index) {
            row.state = state;
            if let Some(summary) = summary {
                row.summary = summary.to_owned();
            }
            self.transcript_cache.invalidate();
            return true;
        }
        false
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

    fn settle_activity_or_remember(
        &mut self,
        id: DisplayId,
        success: bool,
        summary: Option<String>,
    ) {
        self.settle_activity_state_or_remember(
            id,
            if success {
                ActivityState::Success
            } else {
                ActivityState::Failure
            },
            summary,
        );
    }

    fn apply_tool_result_to_display(
        &mut self,
        call_id: &str,
        output: &str,
        is_error: bool,
        output_truncated: bool,
        now_ms: u64,
    ) -> bool {
        let id = self.projector.tool_calls.get(call_id).cloned();
        let index = id
            .as_ref()
            .and_then(|id| self.projector.display_position(id));
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
        for msg in &mut self.msgs {
            if let Msg::Activity(row) = msg {
                if row.id.0.starts_with("retry:") && row.state.is_active() {
                    row.state = crate::display::ActivityState::Success;
                }
            }
        }
    }

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
        if let Some(Msg::Block(block)) = self
            .msgs
            .iter_mut()
            .find(|msg| matches!(msg, Msg::Block(block) if block.id == id))
        {
            if streaming {
                block.content.push_str(text);
                block.copy_source.push_str(text);
                if let Some(unit) = block.unit {
                    self.units.insert(unit, block.copy_source.clone());
                }
            } else {
                block.content = text.to_owned();
                block.copy_source = text.to_owned();
                block.streaming = false;
                if let Some(unit) = block.unit {
                    self.units.insert(unit, block.copy_source.clone());
                }
            }
            self.transcript_cache.invalidate();
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
        self.transcript_cache.invalidate();
    }

    /// Compatibility reducer used while existing `Msg` storage migrates to
    /// the shared display models.
    fn reduce_host_event(&mut self, event: &HostEvent) {
        match &event.kind {
            HostEventKind::UserMessage {
                text,
                source_kind,
                content,
                source,
            } => {
                self.transcript_cache.invalidate();
                if self.status == AgentStatus::Idle {
                    self.stop_thinking();
                }
                let attachments = content
                    .iter()
                    .filter_map(|block| match block {
                        HostContentBlock::Image { label } => Some(format!("[Image: {label}]")),
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
                let is_compaction_checkpoint =
                    matches!(event.surface_op, Some(HostSurfaceOp::Replace { .. }))
                        && matches!(source.producer.as_deref(), Some("compact" | "compaction"));
                if source_kind.as_deref() == Some("user") && attachments.is_empty() {
                    let trailing_thinking = match self.msgs.last() {
                        Some(Msg::Thinking(_)) => self.msgs.pop(),
                        _ => None,
                    };
                    self.msgs.push(Msg::User { text: text.clone() });
                    if let Some(card) = trailing_thinking {
                        self.msgs.push(card);
                    }
                } else if is_compaction_checkpoint {
                    self.push_card(ContentCard {
                        id: event.seq.map_or_else(
                            || DisplayId::correlated("compaction-detail", "legacy"),
                            |seq| DisplayId::event(seq, "compaction-summary"),
                        ),
                        unit: None,
                        header: Some("Compaction summary".into()),
                        content: displayed.clone(),
                        role: CardRole::Detail,
                        tone: DisplayTone::Dim,
                        horizontal_padding: self.config.user_input_padding,
                        copy_source: displayed,
                    });
                } else if source.form.as_deref() == Some("notice") {
                    let notice = source.summary.as_deref().unwrap_or(&displayed);
                    self.push_block(TranscriptBlock {
                        id: event.seq.map_or_else(
                            || DisplayId::correlated("notice", "legacy"),
                            |seq| DisplayId::event(seq, "notice"),
                        ),
                        unit: None,
                        content: notice.chars().take(200).collect(),
                        format: TranscriptFormat::Plain,
                        tone: DisplayTone::Dim,
                        copy_source: notice.to_owned(),
                        streaming: false,
                    });
                } else if source.form.is_some() || !attachments.is_empty() {
                    let role = if source_kind.as_deref() == Some("user") {
                        CardRole::Attachment
                    } else {
                        CardRole::Context
                    };
                    self.push_card(ContentCard {
                        id: event.seq.map_or_else(
                            || DisplayId::correlated("context", "legacy"),
                            |seq| DisplayId::event(seq, "context"),
                        ),
                        unit: None,
                        header: source.form.as_ref().map(|form| format!("Context · {form}")),
                        content: displayed.clone(),
                        role,
                        tone: DisplayTone::Dim,
                        horizontal_padding: self.config.user_input_padding,
                        copy_source: displayed,
                    });
                } else {
                    self.msgs.push(Msg::System {
                        text: text.chars().take(200).collect(),
                    });
                }
            }
            HostEventKind::AssistantChunk {
                text,
                reasoning,
                turn,
                step,
                usage,
            } => {
                self.record_usage(*turn, *step, *usage);
                self.stop_thinking();
                self.settle_retry_activities();
                if !reasoning.is_empty() {
                    self.upsert_reasoning(*turn, *step, reasoning, true);
                }
                if text.is_empty() {
                    return;
                }
                match self.msgs.last_mut() {
                    Some(Msg::Streaming { text: buf }) => {
                        buf.push_str(text);
                        self.transcript_cache.mark_tail_dirty();
                    }
                    _ => {
                        self.msgs.push(Msg::Streaming { text: text.clone() });
                        self.transcript_cache.invalidate();
                    }
                }
            }
            HostEventKind::AssistantMessage {
                text,
                reasoning,
                turn,
                step,
                usage,
                ..
            } => {
                self.record_usage(*turn, *step, *usage);
                self.transcript_cache.invalidate();
                self.stop_thinking();
                self.settle_retry_activities();
                if !reasoning.is_empty() {
                    self.upsert_reasoning(*turn, *step, reasoning, false);
                }
                if matches!(self.msgs.last(), Some(Msg::Streaming { .. })) {
                    self.msgs.pop();
                }
                if !text.is_empty() {
                    self.msgs.push(Msg::Assistant {
                        text: text.clone(),
                        lines: Vec::new(),
                        unit_start: self.next_unit,
                    });
                }
            }
            HostEventKind::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                self.transcript_cache.invalidate();
                self.stop_thinking();
                let start_ms = host_event_time(event);
                let file_call = classify_file_call(name, arguments, self.session_cwd.as_deref());
                let group_at = self
                    .msgs
                    .iter()
                    .rposition(|m| !matches!(m, Msg::Thinking(_)));
                let group_open =
                    group_at.map_or(false, |i| matches!(self.msgs[i], Msg::FileGroup(_)));
                let display_index;
                if let Some((action, file)) = file_call
                    .as_ref()
                    .filter(|(action, _)| action.foldable() && self.config.read_merge)
                {
                    let item = FileItem {
                        action: *action,
                        call_id: call_id.clone(),
                        file: file.clone(),
                        ok: None,
                    };
                    if group_open {
                        let i = group_at.expect("open group index");
                        display_index = i;
                        if let Some(Msg::FileGroup(group)) = self.msgs.get_mut(i) {
                            group.items.push(item);
                        }
                    } else {
                        self.activity_epoch
                            .get_or_insert_with(std::time::Instant::now);
                        self.msgs.push(Msg::FileGroup(FileGroup {
                            items: vec![item],
                            frame: 0,
                            done_since: None,
                            done_from: None,
                        }));
                        display_index = self.msgs.len() - 1;
                    }
                } else {
                    let (display_name, summary) = file_call
                        .map(|(action, file)| (action.label().to_owned(), file))
                        .unwrap_or_else(|| (name.clone(), tool_summary(name, arguments)));
                    self.activity_epoch
                        .get_or_insert_with(std::time::Instant::now);
                    self.msgs.push(Msg::Tool(ToolCard {
                        call_id: call_id.clone(),
                        name: display_name,
                        summary,
                        state: ToolState::Running,
                        frame: 0,
                        start_ms,
                        done_since: None,
                        done_from: None,
                    }));
                    display_index = self.msgs.len() - 1;
                }
                let id = DisplayId::correlated("tool-call", call_id);
                self.projector
                    .tool_calls
                    .insert(call_id.clone(), id.clone());
                self.projector.record_display_position(id, display_index);
                if let Some(pending) = self.projector.take_tool_result(call_id) {
                    self.apply_tool_result_to_display(
                        call_id,
                        &pending.output,
                        pending.is_error,
                        pending.output_truncated,
                        pending.time_ms,
                    );
                    if let Some(seq) = pending.surface_seq {
                        self.projector
                            .record_surface_owner(seq, display_index, false);
                    }
                }
            }
            HostEventKind::ToolResult {
                call_id,
                output,
                is_error,
                output_truncated,
            } => {
                self.start_thinking();
                let now_ms = host_event_time(event);
                if !self.apply_tool_result_to_display(
                    call_id,
                    output,
                    *is_error,
                    *output_truncated,
                    now_ms,
                ) {
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
            }
            HostEventKind::TurnStart => {
                self.start_thinking();
                self.activity_epoch
                    .get_or_insert_with(std::time::Instant::now);
            }
            HostEventKind::TurnEnd {
                reason,
                error_message,
                error_code,
            } => {
                self.transcript_cache.invalidate();
                self.settle_turn(
                    host_event_time(event),
                    matches!(reason.as_deref(), Some("aborted" | "interrupted")),
                );
                match reason.as_deref() {
                    Some("aborted") => self.msgs.push(Msg::System {
                        text: "（已中断）".into(),
                    }),
                    Some("error") => self.msgs.push(Msg::Error {
                        text: match (error_message, error_code) {
                            (Some(message), Some(code)) => format!("{message} ({code})"),
                            (Some(message), None) => message.clone(),
                            _ => "turn error".into(),
                        },
                    }),
                    Some("blocked") => self.msgs.push(Msg::System {
                        text: "（已阻塞）".into(),
                    }),
                    Some("interrupted") => self.msgs.push(Msg::System {
                        text: "（会话异常中断）".into(),
                    }),
                    Some("max-tokens") => self.push_block(TranscriptBlock {
                        id: event.seq.map_or_else(
                            || DisplayId::correlated("turn", "max-tokens"),
                            |seq| DisplayId::event(seq, "turn-outcome"),
                        ),
                        unit: None,
                        content: "（达到模型输出 token 上限）".into(),
                        format: TranscriptFormat::Plain,
                        tone: DisplayTone::Warning,
                        copy_source: "（达到模型输出 token 上限）".into(),
                        streaming: false,
                    }),
                    _ => {}
                }
            }
            HostEventKind::LlmRetry {
                retry_id,
                retry,
                max_retries,
                delay_ms,
                message,
            } => {
                let id = DisplayId::correlated("retry", retry_id);
                let summary = match max_retries {
                    Some(max) => format!("{retry}/{max} · {}ms · {message}", delay_ms),
                    None => format!("{retry} · {}ms · {message}", delay_ms),
                };
                if self.replaying
                    && self
                        .projector
                        .display_position(&id)
                        .is_some_and(|index| index >= self.msgs.len())
                {
                    // The newer retry-started row lives in the saved page.
                    // Defer the older schedule details until saved rows have
                    // been restored and their indexes shifted.
                    self.projector.remember_activity_enrichment(
                        id,
                        PendingActivityEnrichment {
                            summary,
                            start_ms: event.time_ms,
                        },
                    );
                    return;
                }
                self.projector.retries.insert(retry_id.clone(), id.clone());
                let mut row = ActivityRow::root(id, "retry");
                row.state = ActivityState::Waiting;
                row.start_ms = event.time_ms;
                row.summary = summary;
                self.upsert_activity(row);
            }
            HostEventKind::LlmRetryStarted { retry_id, retry } => {
                let id = self
                    .projector
                    .retries
                    .get(retry_id)
                    .cloned()
                    .unwrap_or_else(|| DisplayId::correlated("retry", retry_id));
                self.projector.retries.insert(retry_id.clone(), id.clone());
                let summary = self
                    .projector
                    .display_position(&id)
                    .and_then(|index| self.msgs.get(index))
                    .and_then(|msg| match msg {
                        Msg::Activity(row) => Some(row.summary.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("attempt {retry}"));
                let mut row = ActivityRow::root(id, "retry");
                row.state = ActivityState::Running;
                row.summary = summary;
                self.upsert_activity(row);
            }
            HostEventKind::CommandRun {
                command_id,
                name,
                args,
            } => {
                let id = DisplayId::correlated("command", command_id);
                self.projector
                    .commands
                    .insert(command_id.clone(), id.clone());
                let mut row = ActivityRow::root(id.clone(), format!("/{name}"));
                row.summary = args
                    .clone()
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(120)
                    .collect();
                row.start_ms = event.time_ms;
                self.upsert_activity(row);
            }
            HostEventKind::CommandDone {
                command_id,
                success,
                text,
            } => {
                let id = self
                    .projector
                    .commands
                    .get(command_id)
                    .cloned()
                    .unwrap_or_else(|| DisplayId::correlated("command", command_id));
                self.settle_activity_or_remember(id, *success, text.clone());
            }
            HostEventKind::CodeDispatchStart {
                root_call_id,
                parent_call_id,
                sub_call_id,
                name,
                arguments,
            } => {
                let id = DisplayId::correlated("code-dispatch", sub_call_id);
                let parent_call_id = if parent_call_id.is_empty() {
                    root_call_id
                } else {
                    parent_call_id
                };
                let parent_id = self
                    .projector
                    .nested_calls
                    .get(parent_call_id)
                    .or_else(|| self.projector.tool_calls.get(parent_call_id))
                    .cloned()
                    .unwrap_or_else(|| DisplayId::correlated("tool-call", parent_call_id));
                let depth = self
                    .projector
                    .display_position(&parent_id)
                    .and_then(|index| self.msgs.get(index))
                    .and_then(|msg| match msg {
                        Msg::Activity(parent) => Some(parent.depth.saturating_add(1)),
                        _ => None,
                    })
                    .unwrap_or(1);
                self.projector
                    .nested_calls
                    .insert(sub_call_id.clone(), id.clone());
                let mut row = ActivityRow::root(id, name.clone());
                row.parent_id = Some(parent_id);
                row.depth = depth;
                row.summary = arguments.chars().take(120).collect();
                row.start_ms = event.time_ms;
                self.upsert_activity(row);
            }
            HostEventKind::CodeDispatchEnd {
                sub_call_id,
                is_error,
            } => {
                let id = self
                    .projector
                    .nested_calls
                    .get(sub_call_id)
                    .cloned()
                    .unwrap_or_else(|| DisplayId::correlated("code-dispatch", sub_call_id));
                self.settle_activity_or_remember(id, !*is_error, None);
            }
            HostEventKind::WorkflowRunStart { run_id, name } => {
                let id = DisplayId::correlated("workflow", run_id);
                self.projector.workflows.insert(run_id.clone(), id.clone());
                let mut row = ActivityRow::root(id, "workflow");
                row.summary = name.chars().take(120).collect();
                row.start_ms = event.time_ms;
                self.upsert_activity(row);
            }
            HostEventKind::WorkflowAgentStart {
                run_id,
                member_seq,
                label,
            } => {
                let key = format!("{run_id}:{member_seq}");
                let id = DisplayId::correlated("workflow-agent", &key);
                let mut row = ActivityRow::root(id, "agent");
                row.summary = label.chars().take(120).collect();
                row.parent_id = Some(DisplayId::correlated("workflow", run_id));
                row.depth = 1;
                row.start_ms = event.time_ms;
                self.upsert_activity(row);
            }
            HostEventKind::WorkflowAgentEnd {
                run_id,
                member_seq,
                outcome,
            } => {
                let state = match outcome {
                    HostLifecycleOutcome::Success => ActivityState::Success,
                    HostLifecycleOutcome::Failure => ActivityState::Failure,
                    HostLifecycleOutcome::Cancelled => ActivityState::Cancelled,
                };
                self.settle_activity_state_or_remember(
                    DisplayId::correlated("workflow-agent", &format!("{run_id}:{member_seq}")),
                    state,
                    None,
                );
            }
            HostEventKind::WorkflowRunEnd { run_id, outcome } => {
                let state = match outcome {
                    HostLifecycleOutcome::Success => ActivityState::Success,
                    HostLifecycleOutcome::Failure => ActivityState::Failure,
                    HostLifecycleOutcome::Cancelled => ActivityState::Cancelled,
                };
                self.settle_activity_state_or_remember(
                    DisplayId::correlated("workflow", run_id),
                    state,
                    None,
                );
            }
            HostEventKind::CompactionStart { compaction_id } => {
                let id = DisplayId::correlated("compaction", compaction_id);
                self.projector
                    .compactions
                    .insert(compaction_id.clone(), id.clone());
                let mut row = ActivityRow::root(id, "compacting");
                row.start_ms = event.time_ms;
                self.upsert_activity(row);
            }
            HostEventKind::CompactionSummary {
                compaction_id,
                summary: _,
            } => {
                let id = DisplayId::correlated("compaction", compaction_id);
                if let Some(index) = self.projector.display_position(&id) {
                    if let Some(Msg::Activity(row)) = self.msgs.get_mut(index) {
                        row.summary = "summary ready".into();
                        self.transcript_cache.invalidate();
                    }
                }
            }
            HostEventKind::CompactionEnd {
                compaction_id,
                error,
            } => {
                self.settle_activity_or_remember(
                    DisplayId::correlated("compaction", compaction_id),
                    error.is_none(),
                    error.clone(),
                );
            }
            HostEventKind::GoalChange { summary } => {
                self.goal = (!summary.is_empty()).then(|| summary.clone())
            }
            HostEventKind::PlanMode { mode } => {
                self.plan_mode = (!mode.is_empty()).then(|| mode.clone())
            }
            HostEventKind::AgentPresetSelected { preset } => {
                let is_newest = match (event.seq, self.current_mode_seq) {
                    (Some(incoming), Some(current)) => incoming >= current,
                    (Some(_), None) | (None, None) => true,
                    (None, Some(_)) => false,
                };
                if is_newest && !preset.is_empty() {
                    self.current_mode = Some(preset.clone());
                    self.current_mode_seq = event.seq;
                }
            }
            HostEventKind::SessionState { event_type } => {
                self.session_state_events.insert(event_type.clone());
            }
            HostEventKind::SessionTitle { title } => {
                self.session_title = title.clone();
            }
            HostEventKind::StepStart { .. }
            | HostEventKind::StepEnd { .. }
            | HostEventKind::TodoWrite { .. }
            | HostEventKind::AuditOnly { .. }
            | HostEventKind::Unknown { .. } => {}
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
        let existing_displays = self.projector.display_ids();
        // Token totals should absorb older pages, but replacement bookkeeping
        // must keep pointing at the newest loaded request.
        let newest_usage_sample = self.last_usage_sample;
        let saved = std::mem::take(&mut self.msgs);
        self.replaying = true;
        for event in events {
            self.apply_host_event(event);
        }
        self.replaying = false;
        if newest_usage_sample.is_some() {
            self.last_usage_sample = newest_usage_sample;
        }
        let mut added = self.msgs.len();
        let dangling = matches!(self.msgs.last(), Some(Msg::Streaming { .. }))
            && matches!(saved.first(), Some(Msg::Assistant { .. }));
        if dangling {
            self.msgs.pop();
            added -= 1;
        }
        self.msgs.extend(saved);
        self.projector
            .shift_selected_owners(&existing_owners, added);
        self.projector
            .shift_selected_displays(&existing_displays, added);
        self.apply_pending_activity_enrichments();
        self.transcript_cache.invalidate();
        self.transcript_cache.tail_dirty = false;
        self.transcript_cache.prepend_anchor = Some(self.transcript_cache.lines.len());
        added
    }
}

/// Capture the settle transition when a file group's last pending item
/// settles: the bullet animates from the breathing color toward umber/red
/// instead of snapping.
fn settle_group(group: &mut FileGroup, from: Color) {
    if !group.pending() && group.done_since.is_none() {
        group.done_since = Some(std::time::Instant::now());
        group.done_from = Some(from);
    }
}

/// Exit code from the `[exit code: N]` marker in shell tool output.
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

/// Short summary for a tool card: the shell command, a read target, or
/// trimmed raw arguments.
fn tool_summary(name: &str, arguments: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(arguments).ok();
    if let Some(parsed) = &parsed {
        if let Some(command) = parsed.get("command").and_then(Value::as_str) {
            return trim_to(command, 120);
        }
        if let Some(path) = parsed
            .get("file_path")
            .or_else(|| parsed.get("filePath"))
            .and_then(Value::as_str)
        {
            return path.to_string();
        }
        if let Some(workdir) = parsed.get("workdir").and_then(Value::as_str) {
            if let Some(command) = parsed.get("command").and_then(Value::as_str) {
                return format!("{command} (in {workdir})");
            }
        }
    }
    let _ = name;
    trim_to(arguments, 120)
}

fn trim_to(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out.replace(['\r', '\n'], " ")
}

/// Drive the breathing/transition animation clock. Returns true while any
/// running indicator is visible or a settle transition is still animating
/// (the caller uses it to schedule a redraw). Colors are baked into the
/// cached lines, so animation invalidates the cache — the renderer rebuilds
/// with fresh colors on the next draw.
pub fn tick_spinners(state: &mut AppState, now: std::time::Instant) -> bool {
    let any_pending = state.msgs.iter().any(|m| match m {
        Msg::Tool(card) => card.state == ToolState::Running,
        Msg::FileGroup(group) => group.pending(),
        Msg::Thinking(card) => card.state == ThinkState::Running,
        Msg::Activity(row) => row.state.is_active(),
        Msg::Block(block) => block.streaming,
        _ => false,
    }) || matches!(state.msgs.last(), Some(Msg::Streaming { .. }))
        || state.working
        || state.status == AgentStatus::Running;

    if any_pending {
        if state.activity_epoch.is_none() {
            state.activity_epoch = Some(now);
        }
    } else {
        state.activity_epoch = None;
    }

    // Settle transitions keep animating briefly after the task ends.
    let transitioning = state.msgs.iter().any(|m| match m {
        Msg::Tool(card) => card
            .done_since
            .map_or(false, |t| t.elapsed().as_millis() < SETTLE_TRANSITION_MS),
        Msg::FileGroup(group) => group
            .done_since
            .map_or(false, |t| t.elapsed().as_millis() < SETTLE_TRANSITION_MS),
        Msg::Thinking(card) => card
            .done_since
            .map_or(false, |t| t.elapsed().as_millis() < SETTLE_TRANSITION_MS),
        _ => false,
    });

    if any_pending || transitioning {
        state.transcript_cache.valid = false;
    }
    any_pending || transitioning
}

fn host_event_time(event: &HostEvent) -> u64 {
    event.time_ms.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    })
}

/// Classify built-in file tools and str_replace_editor commands into the
/// common activity vocabulary. Editor paths are absolute by schema, so make
/// them workspace-relative when possible before they reach the display model.
fn classify_file_call(
    name: &str,
    arguments: &str,
    workspace: Option<&str>,
) -> Option<(FileAction, String)> {
    let parsed: Value = serde_json::from_str(arguments).ok()?;
    let ordinary = match name {
        "read" | "read_text" | "read_image" => Some(FileAction::Read),
        "edit" | "write" => Some(FileAction::Edit),
        _ => None,
    };
    if let Some(action) = ordinary {
        let path = parsed
            .get("file_path")
            .or_else(|| parsed.get("filePath"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Some((action, path.to_owned()));
    }
    if name != "str_replace_editor" {
        return None;
    }
    let action = match parsed.get("command").and_then(Value::as_str)? {
        "view" => FileAction::View,
        "create" => FileAction::Create,
        "str_replace" => FileAction::Replace,
        "insert" => FileAction::Insert,
        _ => return None,
    };
    let path = parsed
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some((action, workspace_relative_path(path, workspace)))
}

fn workspace_relative_path(path: &str, workspace: Option<&str>) -> String {
    let normalize = |value: &str| value.trim_end_matches(['/', '\\']).replace('\\', "/");
    let normalized_path = normalize(path);
    let Some(workspace) = workspace else {
        return normalized_path;
    };
    let normalized_workspace = normalize(workspace);
    if normalized_workspace.is_empty() {
        return normalized_path;
    }
    let path_parts: Vec<&str> = normalized_path.split('/').collect();
    let workspace_parts: Vec<&str> = normalized_workspace.split('/').collect();
    let inside_workspace = path_parts.len() >= workspace_parts.len()
        && path_parts
            .iter()
            .zip(&workspace_parts)
            .all(|(path, workspace)| path.eq_ignore_ascii_case(workspace));
    if !inside_workspace {
        return normalized_path;
    }
    let relative = &path_parts[workspace_parts.len()..];
    if relative.is_empty() {
        ".".to_owned()
    } else {
        relative.join("/")
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
            !tick_spinners(&mut s, std::time::Instant::now()),
            "animation clock stops after the settle transition"
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
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "running drives redraws"
        );
        assert!(s.activity_epoch.is_some(), "epoch set while running");
        assert!(!s.transcript_cache.valid, "animation invalidates the cache");
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
        assert!(
            tick_spinners(&mut s, std::time::Instant::now()),
            "transition animates"
        );
        // Long-settled: the clock stops.
        card.done_since = Some(
            std::time::Instant::now()
                - std::time::Duration::from_millis(SETTLE_TRANSITION_MS as u64 + 10),
        );
        s.msgs[0] = Msg::Tool(card);
        s.transcript_cache.valid = true;
        assert!(
            !tick_spinners(&mut s, std::time::Instant::now()),
            "settled card stops redraws"
        );
        assert!(
            s.transcript_cache.valid,
            "no invalidation when nothing animates"
        );
        assert!(s.activity_epoch.is_none(), "epoch cleared when idle");
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
        assert!(!s.transcript_cache.valid, "animation invalidates the cache");
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
