//! Session-event projection into renderable message cards (design §3.2/§3.3).
//!
//! The bridge forwards raw session events; this module folds them into a
//! small owned message model. Tool cards are created on `tool/call` and
//! finalized on `tool/result` (D21/D22); assistant text streams in through
//! `assistant/chunk` and is replaced by the assembled `assistant/message`.

use serde_json::Value;

use ratatui::style::Color;

use crate::config::{Config, Theme};
use crate::render::{render_markdown, RenderLine, RenderOptions};

/// Snapshot guard: only this many surface events are replayed. This is the
/// CLIENT's local render-cache budget, independent of the bridge's wire
/// budget (`SNAPSHOT_CAP` = 600 in bridge/src/index.js): the bridge keeps
/// the welcome frame small, this cap protects the cache from pathological
/// logs that arrive through other paths. Do not "unify" the two values —
/// they guard different layers.
const SNAPSHOT_SURFACE_CAP: usize = 2000;

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
            let l = |a: u8, b: u8| {
                (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8
            };
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
    lerp_color(from, to, elapsed.as_millis() as f64 / SETTLE_TRANSITION_MS as f64)
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
    Done { ok: bool, lines: usize, duration_ms: u64 },
}

/// A merged group of consecutive read/edit calls rendered on one line:
/// `read a.rs, b.rs; edit c.rs`. While a read is pending the line shows the
/// spinner; while an edit is pending the finished reads keep their own line
/// with a trailing `;` and the running edit gets a spinner line below.
#[derive(Debug, Clone)]
pub struct FileGroup {
    pub reads: Vec<ReadItem>,
    pub edits: Vec<EditItem>,
    pub frame: usize,
    /// Settle transition (whole group): captured when the last pending item
    /// settles, animated toward umber/red instead of snapping.
    pub done_since: Option<std::time::Instant>,
    pub done_from: Option<Color>,
}

#[derive(Debug, Clone)]
pub struct ReadItem {
    pub call_id: String,
    pub file: String,
    /// None = still pending.
    pub ok: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct EditItem {
    pub call_id: String,
    pub file: String,
    /// None = still pending.
    pub ok: Option<bool>,
}

impl FileGroup {
    pub fn pending(&self) -> bool {
        self.reads.iter().any(|i| i.ok.is_none())
            || self.edits.iter().any(|i| i.ok.is_none())
    }
}

/// Screen rows a file group renders. MUST match `ui::msg_lines` exactly —
/// copy-mode row math (global_row) depends on it. Failed items render one
/// line per DISTINCT file (repeats collapse into `name xN`).
pub fn file_group_line_count(group: &FileGroup) -> usize {
    let distinct_failed = |items: &[&String]| -> usize {
        let mut seen = std::collections::HashSet::new();
        let mut n = 0usize;
        for f in items {
            if seen.insert(f.as_str()) {
                n += 1;
            }
        }
        n
    };
    let pending_reads = group.reads.iter().filter(|i| i.ok.is_none()).count();
    let failed_reads = distinct_failed(
        &group
            .reads
            .iter()
            .filter(|i| i.ok == Some(false))
            .map(|i| &i.file)
            .collect::<Vec<_>>(),
    );
    let ok_reads = group.reads.iter().filter(|i| i.ok == Some(true)).count();
    let pending_edits = group.edits.iter().filter(|i| i.ok.is_none()).count();
    let failed_edits = distinct_failed(
        &group
            .edits
            .iter()
            .filter(|i| i.ok == Some(false))
            .map(|i| &i.file)
            .collect::<Vec<_>>(),
    );
    let ok_edits = group.edits.iter().filter(|i| i.ok == Some(true)).count();
    if pending_reads > 0 {
        // breathing read line + failed reads
        1 + failed_reads
    } else if pending_edits > 0 {
        // merged ok reads? + failed reads + breathing edit line + failed edits
        usize::from(ok_reads > 0) + failed_reads + 1 + failed_edits
    } else {
        // folded ok line? + failed reads + failed edits
        usize::from(ok_reads > 0 || ok_edits > 0) + failed_reads + failed_edits
    }
}

/// One renderable message row/card in the transcript.
#[derive(Debug, Clone)]
pub enum Msg {
    /// User message, shown verbatim (D20).
    User { text: String },
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
    Streaming { text: String },
    Tool(ToolCard),
    /// Model-thinking phase (between user send / tool results and the next
    /// visible activity), rendered like a tool card: breathing bullet while
    /// the model thinks, green once the phase completes.
    Thinking(ThinkingCard),
    /// Merged consecutive read/edit calls on one line.
    FileGroup(FileGroup),
    /// System / lifecycle notices (session start, compaction …).
    System { text: String },
    Error { text: String },
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
            let custom = if trimmed.is_empty() { None } else { Some(trimmed) };
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
    pub session_id: Option<String>,
    pub status: AgentStatus,
    pub provider: Option<String>,
    pub model: Option<String>,
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
    next_unit: u64,
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
    /// Render cache: the transcript lines without copy-mode overlay.
    /// Rebuilt only when messages change (rendering perf guard).
    pub render_cache: Vec<ratatui::text::Line<'static>>,
    pub cache_valid: bool,
    /// Incremental streaming updates: chunks extend the last message and
    /// only its cached tail needs re-rendering (perf: streaming chunks are
    /// high-frequency and must not rebuild the whole transcript).
    pub tail_dirty: bool,
    /// Lines the last message contributed to `render_cache` (incl. its
    /// trailing gap row) — used to splice the tail in place.
    pub cache_tail_len: usize,
    /// Spinner frame for the streaming indicator.
    pub stream_frame: usize,
    /// Earliest event seq among the loaded transcript (history paging base).
    pub min_seq: Option<u64>,
    /// A scroll-back history request is in flight.
    pub history_loading: bool,
    /// No older events exist (the whole log is loaded).
    pub history_exhausted: bool,
    /// Cache line count before a history prepend; the next rebuild adjusts
    /// the scroll offset by the added lines to keep the viewport anchored.
    pub prepend_line_anchor: Option<usize>,
    /// Content width of the last transcript render — copy-mode row math
    /// uses it to mirror wrapped user blocks.
    pub render_width: usize,
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
            session_id: None,
            status: AgentStatus::Idle,
            provider: None,
            model: None,
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
            render_cache: Vec::new(),
            cache_valid: false,
            tail_dirty: false,
            cache_tail_len: 0,
            stream_frame: 0,
            min_seq: None,
            history_loading: false,
            history_exhausted: false,
            prepend_line_anchor: None,
            render_width: 80,
            working: false,
            replaying: false,
            activity_epoch: None,
        }
    }
}

impl AppState {
    /// Render options derived from the live config.
    pub fn render_options(&self) -> RenderOptions {
        RenderOptions {
            expanded: self.expanded.clone(),
            collapse_rows: self.config.atomic_collapse_rows,
            mermaid_enabled: self.config.mermaid_enabled,
        }
    }

    pub fn theme(&self) -> crate::config::Theme {
        self.config.theme()
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
                self.cache_valid = false;
            }
            _ => {
                self.msgs.push(Msg::Thinking(ThinkingCard {
                    state: ThinkState::Running,
                    count: 1,
                    done_since: None,
                    done_from: None,
                }));
                self.cache_valid = false;
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
                self.cache_valid = false;
            }        }
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
    pub fn settle_turn(&mut self, now_ms: u64) {
        let breath_now = breathing_color(&self.config.theme(), self.breath_phase());
        for msg in self.msgs.iter_mut() {
            match msg {
                Msg::Tool(card) => {
                    if card.state == ToolState::Running {
                        let duration_ms = now_ms.saturating_sub(card.start_ms);
                        card.state = ToolState::Done { ok: false, lines: 0, duration_ms };
                        card.done_since = Some(std::time::Instant::now());
                        card.done_from = Some(breath_now);
                    }
                }
                Msg::FileGroup(group) => {
                    for item in group.reads.iter_mut() {
                        if item.ok.is_none() {
                            item.ok = Some(false);
                        }
                    }
                    for item in group.edits.iter_mut() {
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
                _ => {}
            }
        }
        // A dangling streaming tail would keep its breathing bullet forever:
        // finalize it into a regular assistant message.
        if matches!(self.msgs.last(), Some(Msg::Streaming { .. })) {
            if let Some(Msg::Streaming { text }) = self.msgs.pop() {
                if !text.is_empty() {
                    let unit_start = self.next_unit;
                    let options = self.render_options();
                    let lines = render_markdown(
                        &text,
                        &self.config.theme(),
                        &mut self.next_unit,
                        &options,
                        &mut self.units,
                    );
                    self.msgs.push(Msg::Assistant { text, lines, unit_start });
                }
            }
        }
        self.working = false;
        self.cache_valid = false;
    }
}

impl AppState {
    /// Drop the whole transcript (used when attaching to another session).
    pub fn reset_transcript(&mut self) {
        self.msgs.clear();
        self.units.clear();
        self.expanded.clear();
        self.next_unit = 0;
        self.snapshot_truncated = false;
        self.render_cache.clear();
        self.cache_valid = false;
        self.tail_dirty = false;
        self.cache_tail_len = 0;
        self.min_seq = None;
        self.history_loading = false;
        self.history_exhausted = false;
        self.prepend_line_anchor = None;
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
    /// Toggle a collapsed atomic block (D13). Re-renders the owning
    /// assistant message so its line list matches the new window.
    pub fn toggle_expand(&mut self, unit: u64) {
        if !self.units.contains_key(&unit) {
            return;
        }
        if self.expanded.contains(&unit) {
            self.expanded.remove(&unit);
        } else {
            self.expanded.insert(unit);
        }
        self.cache_valid = false;
        // Re-render every assistant message that owns this unit. The message
        // reuses its unit range so copy-mode references stay valid.
        let options = self.render_options();
        for msg in self.msgs.iter_mut() {
            if let Msg::Assistant { text, lines, unit_start } = msg {
                if lines.iter().any(|l| l.unit == unit) {
                    let start = *unit_start;
                    let saved = self.next_unit;
                    self.next_unit = start;
                    *lines = render_markdown(
                        text,
                        &self.config.theme(),
                        &mut self.next_unit,
                        &options,
                        &mut self.units,
                    );
                    self.next_unit = saved.max(self.next_unit);
                    break;
                }
            }
        }
    }
}

impl AppState {
    /// Apply one bridge message payload (welcome / snapshot / event / status).
    pub fn apply(&mut self, kind: &str, data: &Value) {
        match kind {
            "welcome" => {
                let new_id = data.get("sessionId").and_then(Value::as_str).map(String::from);
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
                self.provider = data.get("provider").and_then(Value::as_str).map(String::from);
                self.model = data.get("model").and_then(Value::as_str).map(String::from);
            }
            "snapshot" => {
                let events = data.get("events").and_then(Value::as_array).cloned().unwrap_or_default();
                let bridge_truncated = data
                    .get("truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut surface: Vec<Value> = events
                    .into_iter()
                    .filter(|e| is_surface_event(e))
                    .collect();
                let truncated = bridge_truncated || surface.len() > SNAPSHOT_SURFACE_CAP;
                if truncated {
                    surface = surface.split_off(surface.len().saturating_sub(SNAPSHOT_SURFACE_CAP));
                    self.snapshot_truncated = true;
                    self.msgs.push(Msg::System {
                        text: "（历史较长，仅回放最近消息）".into(),
                    });
                }
                // Replay suppresses Thinking rows: history is reconstructed
                // as-is; only the live tail shows per-phase indicators.
                self.replaying = true;
                for event in surface {
                    self.apply_event(&event);
                }
                self.replaying = false;
                // Mid-turn attach (the tail ended inside a thinking phase):
                // show a live Thinking row until the next activity settles it.
                if self.working {
                    self.start_thinking();
                }
                // Lazy scroll-back: exhausted only when the whole log is here.
                self.history_exhausted = !truncated;
                self.history_loading = false;
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

    /// Apply one bridge event. Cache invalidation is per-arm: only events
    /// that change the transcript structure force a full rebuild; streaming
    /// chunks mark just the tail dirty; high-frequency events that do not
    /// render (step/*, todo/write, …) invalidate nothing (perf: the render
    /// loop must not rebuild the whole transcript per chunk).
    pub fn apply_event(&mut self, event: &Value) {
        if let Some(seq) = event.get("seq").and_then(Value::as_u64) {
            self.min_seq = Some(self.min_seq.map_or(seq, |m| m.min(seq)));
        }
        let Some(ty) = event.get("type").and_then(Value::as_str) else { return };
        let Some(data) = event.get("data") else { return };
        match ty {
            "user/message" => {
                self.cache_valid = false;
                // The echo of the user's own message must not settle the
                // Thinking row — it started showing at send time. Only clear
                // it when the agent is not actually working.
                if self.status == AgentStatus::Idle {
                    self.stop_thinking();
                }
                let text = data.get("content").map(collect_text).unwrap_or_default();
                // Only surface direct human prompts; synthetic injected context
                // (instructions, notices …) renders as a dim system line.
                let source_kind = data
                    .get("source")
                    .and_then(|s| s.get("kind"))
                    .and_then(Value::as_str);
                if source_kind == Some("user") {
                    // The client optimistically shows a Thinking card the
                    // moment the user sends a message, before the bridge
                    // echoes the `user/message` event. Move the echo BEFORE
                    // that trailing card so the transcript reads
                    // "user message → thinking" rather than
                    // "thinking → user message".
                    let trailing_thinking = match self.msgs.last() {
                        Some(Msg::Thinking(_)) => self.msgs.pop(),
                        _ => None,
                    };
                    self.msgs.push(Msg::User { text });
                    if let Some(card) = trailing_thinking {
                        self.msgs.push(card);
                    }
                } else {
                    self.msgs.push(Msg::System {
                        text: text.chars().take(200).collect(),
                    });
                }
            }
            "assistant/chunk" => {
                self.stop_thinking();
                if let Some(text) = chunk_text(data) {
                    match self.msgs.last_mut() {
                        Some(Msg::Streaming { text: buf }) => {
                            buf.push_str(&text);
                            // Only the cached tail changed: splice it, don't
                            // rebuild the transcript.
                            self.tail_dirty = true;
                        }
                        _ => {
                            self.msgs.push(Msg::Streaming { text });
                            self.cache_valid = false;
                        }
                    }
                }
            }
            "assistant/message" => {
                self.cache_valid = false;
                self.stop_thinking();
                let text = data
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .map(collect_text)
                    .unwrap_or_default();
                // Replace any dangling streaming segment.
                match self.msgs.last_mut() {
                    Some(Msg::Streaming { .. }) => {
                        self.msgs.pop();
                    }
                    _ => {}
                }
                if !text.is_empty() {
                    // Pre-render markdown with source mapping (D8, §2.1).
                    let unit_start = self.next_unit;
                    let options = self.render_options();
                    let lines = render_markdown(
                        &text,
                        &self.config.theme(),
                        &mut self.next_unit,
                        &options,
                        &mut self.units,
                    );
                    self.msgs.push(Msg::Assistant {
                        text,
                        lines,
                        unit_start,
                    });
                }
            }
            "tool/call" => {
                self.cache_valid = false;
                self.stop_thinking();
                let call_id = data.get("callId").and_then(Value::as_str).unwrap_or("").to_string();
                let name = data.get("name").and_then(Value::as_str).unwrap_or("tool").to_string();
                let arguments = data.get("arguments").and_then(Value::as_str).unwrap_or("");
                let start_ms = event_time(data, self);
                // File-group merging: consecutive read/edit calls join the
                // line above (`read a, b; edit c`), regardless of pending
                // state; any other message breaks the group.
                let is_read = name == "read" || name == "read_text" || name == "read_image";
                let is_edit = name == "edit" || name == "write";
                // Thinking rows are phase markers between calls, not group
                // members — look past them to find an open file group.
                let group_at = self
                    .msgs
                    .iter()
                    .rposition(|m| !matches!(m, Msg::Thinking(_)));
                let group_open = group_at.map_or(false, |i| matches!(self.msgs[i], Msg::FileGroup(_)));
                if (is_read || is_edit) && self.config.read_merge && group_open {
                    let i = group_at.unwrap();
                    if let Some(Msg::FileGroup(group)) = self.msgs.get_mut(i) {
                        if is_read {
                            group.reads.push(ReadItem {
                                call_id,
                                file: read_target(arguments),
                                ok: None,
                            });
                        } else {
                            group.edits.push(EditItem {
                                call_id,
                                file: read_target(arguments),
                                ok: None,
                            });
                        }
                    }
                } else if (is_read || is_edit) && self.config.read_merge {
                    let mut group = FileGroup {
                        reads: Vec::new(),
                        edits: Vec::new(),
                        frame: 0,
                        done_since: None,
                        done_from: None,
                    };
                    if is_read {
                        group.reads.push(ReadItem {
                            call_id,
                            file: read_target(arguments),
                            ok: None,
                        });
                    } else {
                        group.edits.push(EditItem {
                            call_id,
                            file: read_target(arguments),
                            ok: None,
                        });
                    }
                    self.activity_epoch.get_or_insert_with(std::time::Instant::now);
                    self.msgs.push(Msg::FileGroup(group));
                } else {
                    let summary = tool_summary(&name, arguments);
                    self.activity_epoch.get_or_insert_with(std::time::Instant::now);
                    self.msgs.push(Msg::Tool(ToolCard {
                        call_id,
                        name,
                        summary,
                        state: ToolState::Running,
                        frame: 0,
                        start_ms,
                        done_since: None,
                        done_from: None,
                    }));
                }
            }
            "tool/result" => {
                self.cache_valid = false;
                // After a tool result the model thinks again before the next
                // chunk/tool call — a fresh Thinking row.
                self.start_thinking();
                // Breathing color at settle time — the transition starts here.
                let breath_now = breathing_color(&self.config.theme(), self.breath_phase());
                let call_id = data
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .and_then(|blocks| blocks.first())
                    .and_then(|b| b.get("toolCallId"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let output_text = data
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .and_then(|blocks| blocks.first())
                    .and_then(|b| b.get("content"))
                    .map(collect_text)
                    .unwrap_or_default();
                let lines = output_text.lines().count();
                let ok = exit_marker(&output_text) == 0 && !is_error_result(data);
                let now_ms = event_time(data, self);
                // File groups settle their read/edit item first; Thinking
                // rows between calls are skipped (phase markers).
                let mut settled_group = false;
                for msg in self.msgs.iter_mut().rev() {
                    if matches!(msg, Msg::Thinking(_)) {
                        continue;
                    }
                    if let Msg::FileGroup(group) = msg {
                        if let Some(item) = group.reads.iter_mut().find(|i| i.call_id == call_id) {
                            item.ok = Some(ok);
                            settle_group(group, breath_now);
                            settled_group = true;
                            break;
                        }
                        if let Some(item) = group.edits.iter_mut().find(|i| i.call_id == call_id) {
                            item.ok = Some(ok);
                            settle_group(group, breath_now);
                            settled_group = true;
                            break;
                        }
                    }
                    // Groups absorb every consecutive read/edit; stop at the
                    // first non-file message.
                    break;
                }
                if settled_group {
                    return;
                }
                // Otherwise find the matching tool card.
                for msg in self.msgs.iter_mut().rev() {
                    if let Msg::Tool(card) = msg {
                        if card.call_id == call_id && card.state == ToolState::Running {
                            let duration_ms = now_ms.saturating_sub(card.start_ms);
                            card.state = ToolState::Done { ok, lines, duration_ms };
                            // Capture the settle transition: the running
                            // bullet animates toward the done color instead
                            // of snapping.
                            card.done_since = Some(std::time::Instant::now());
                            card.done_from = Some(breath_now);
                            break;
                        }
                    }
                }
            }
            "turn/start" => {
                // The turn begins: nothing visible yet → Thinking row.
                self.start_thinking();
                self.activity_epoch.get_or_insert_with(std::time::Instant::now);
            }
            "turn/end" => {
                self.cache_valid = false;
                // Settle every still-running indicator (Esc interrupt leaves
                // tool/read/thinking bullets without their result events).
                let now_ms = event_time(data, self);
                self.settle_turn(now_ms);
                // Surface aborted/error turns as a dim notice (design §3.5).
                let reason = data.get("reason").and_then(|r| r.get("kind")).and_then(Value::as_str);
                match reason {
                    Some("aborted") => self.msgs.push(Msg::System { text: "（已中断）".into() }),
                    Some("error") => self.msgs.push(Msg::Error {
                        text: "turn error".into(),
                    }),
                    Some("blocked") => self.msgs.push(Msg::System { text: "（已阻塞）".into() }),
                    _ => {}
                }
            }
            "session/title" => {
                // Title row below the status bar: update in place; the
                // transcript cache is untouched (the row is drawn outside
                // the transcript area).
                self.session_title = data.get("title").and_then(Value::as_str).map(String::from);
            }
            // step/*, todo/write, …: not rendered, don't touch the cache.
            _ => {}
        }
    }

    /// Prepend one page of older history events to the transcript (lazy
    /// scroll-back). Replays the events in order into a scratch list, then
    /// stitches it in front of the existing messages and marks the cache
    /// dirty; `prepend_line_anchor` lets the renderer keep the viewport on
    /// the previously visible content.
    pub fn prepend_events(&mut self, events: &[Value]) -> usize {
        let saved = std::mem::replace(&mut self.msgs, Vec::new());
        // History prepend reconstructs older turns without Thinking rows.
        self.replaying = true;
        for event in events {
            self.apply_event(event);
        }
        self.replaying = false;
        let mut added = self.msgs.len();
        // A page that ended mid-stream leaves a dangling Streaming segment
        // before the assembled Assistant message — drop the duplicate.
        let dangling = matches!(self.msgs.last(), Some(Msg::Streaming { .. }))
            && matches!(saved.first(), Some(Msg::Assistant { .. }));
        if dangling {
            self.msgs.pop();
            added -= 1;
        }
        self.msgs.extend(saved);
        self.cache_valid = false;
        self.tail_dirty = false;
        self.prepend_line_anchor = Some(self.render_cache.len());
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

fn is_surface_event(event: &Value) -> bool {
    matches!(
        event.get("type").and_then(Value::as_str),
        Some(
            "user/message"
                | "assistant/message"
                | "tool/call"
                | "tool/result"
                | "turn/start"
                | "turn/end"
                | "todo/write"
        )
    )
}

/// Concatenate text blocks of a content array.
fn collect_text(content: &Value) -> String {
    content
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| {
                    b.get("type")
                        .and_then(Value::as_str)
                        .filter(|t| *t == "text")
                        .and_then(|_| b.get("text"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Extract a text delta from one stream chunk.
fn chunk_text(data: &Value) -> Option<String> {
    let chunk = data.get("chunk")?;
    match chunk.get("type")?.as_str()? {
        "text-delta" => chunk.get("text")?.as_str().map(String::from),
        _ => None,
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

fn is_error_result(data: &Value) -> bool {
    data.get("error").is_some()
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
        state.cache_valid = false;
    }
    any_pending || transitioning
}

fn event_time(event: &Value, _state: &AppState) -> u64 {
    event.get("time").and_then(Value::as_u64).unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    })
}

fn read_target(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|v| {
            v.get("file_path")
                .or_else(|| v.get("filePath"))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default()
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
        s.apply_event(&event("user/message", serde_json::json!({
            "content": [{"type": "text", "text": "原样展示 # not markdown"}],
            "source": {"kind": "user"}
        })));
        assert!(matches!(&s.msgs[0], Msg::User { text } if text == "原样展示 # not markdown"));
    }

    #[test]
    fn synthetic_context_is_dim() {
        let mut s = AppState::default();
        s.apply_event(&event("user/message", serde_json::json!({
            "content": [{"type": "text", "text": "injected"}],
            "source": {"kind": "plugin", "plugin": "x"}
        })));
        assert!(matches!(&s.msgs[0], Msg::System { .. }));
    }

    #[test]
    fn user_message_precedes_the_optimistic_thinking_card() {
        let mut s = AppState::default();
        // The client shows this placeholder the moment the user sends a
        // message, before the bridge echoes the `user/message` event.
        s.start_thinking();
        assert!(matches!(s.msgs.last(), Some(Msg::Thinking(_))));
        s.apply_event(&event("user/message", serde_json::json!({
            "content": [{"type": "text", "text": "你好"}],
            "source": {"kind": "user"}
        })));
        assert_eq!(s.msgs.len(), 2);
        assert!(matches!(&s.msgs[0], Msg::User { text } if text == "你好"));
        assert!(matches!(&s.msgs[1], Msg::Thinking(_)));
    }

    #[test]
    fn chunks_stream_then_assemble() {
        let mut s = AppState::default();
        s.apply_event(&event("assistant/chunk", serde_json::json!({
            "chunk": {"type": "text-delta", "index": 0, "text": "你好"}
        })));
        s.apply_event(&event("assistant/chunk", serde_json::json!({
            "chunk": {"type": "text-delta", "index": 0, "text": "，世界"}
        })));
        assert!(matches!(&s.msgs[0], Msg::Streaming { text } if text == "你好，世界"));
        s.apply_event(&event("assistant/message", serde_json::json!({
            "message": {"content": [{"type": "text", "text": "你好，世界"}]}
        })));
        assert_eq!(s.msgs.len(), 1);
        assert!(matches!(&s.msgs[0], Msg::Assistant { text, .. } if text == "你好，世界"));
    }

    #[test]
    fn tool_card_lifecycle_exit_code() {
        let mut s = AppState::default();
        s.apply_event(&event("tool/call", serde_json::json!({
            "callId": "c1", "name": "bash",
            "arguments": "{\"command\": \"npm run build\"}"
        })));
        assert!(matches!(&s.msgs[0], Msg::Tool(card)
            if card.summary == "npm run build" && card.state == ToolState::Running));
        s.apply_event(&event("tool/result", serde_json::json!({
            "message": {"content": [{
                "type": "tool-result",
                "toolCallId": "c1",
                "content": [{"type": "text", "text": "line1\nline2\n[exit code: 1]"}]
            }]}
        })));
        assert!(matches!(&s.msgs[0], Msg::Tool(card)
            if card.state == ToolState::Done { ok: false, lines: 3, duration_ms: 0 }));
    }

    #[test]
    fn read_edit_calls_merge_into_one_group() {
        let mut s = AppState::default();
        for (seq, id, name, path) in [
            (1, "r1", "read", "src/input.rs"),
            (2, "r2", "read", "src/foo.rs"),
            (3, "e1", "edit", "src/ui.rs"),
        ] {
            s.apply_event(&event_seq("tool/call", seq, serde_json::json!({
                "callId": id, "name": name,
                "arguments": format!("{{\"file_path\": \"{path}\"}}")
            })));
        }
        assert_eq!(s.msgs.len(), 1, "consecutive read/edit calls merge");
        let Msg::FileGroup(group) = &s.msgs[0] else { panic!("FileGroup expected") };
        assert_eq!(group.reads.len(), 2);
        assert_eq!(group.edits.len(), 1);
        // A non-file tool breaks the group; the next read starts a new one.
        s.apply_event(&event_seq("tool/call", 4, serde_json::json!({
            "callId": "b1", "name": "bash", "arguments": "{}"
        })));
        s.apply_event(&event_seq("tool/call", 5, serde_json::json!({
            "callId": "r3", "name": "read",
            "arguments": "{\"file_path\": \"src/next.rs\"}"
        })));
        assert_eq!(s.msgs.len(), 3);
        assert!(matches!(&s.msgs[2], Msg::FileGroup(g) if g.reads.len() == 1 && g.edits.is_empty()));
    }

    #[test]
    fn tool_result_settles_read_and_edit() {
        let mut s = AppState::default();
        s.apply_event(&event_seq("tool/call", 1, serde_json::json!({
            "callId": "r1", "name": "read",
            "arguments": "{\"file_path\": \"a.rs\"}"
        })));
        s.apply_event(&event_seq("tool/call", 2, serde_json::json!({
            "callId": "e1", "name": "edit",
            "arguments": "{\"file_path\": \"b.rs\"}"
        })));
        s.apply_event(&event_seq("tool/result", 3, serde_json::json!({
            "message": {"content": [{
                "type": "tool-result", "toolCallId": "r1",
                "content": [{"type": "text", "text": "content"}]
            }]}
        })));
        s.apply_event(&event_seq("tool/result", 4, serde_json::json!({
            "message": {"content": [{
                "type": "tool-result", "toolCallId": "e1",
                "content": [{"type": "text", "text": "[exit code: 1]"}]
            }]}
        })));
        assert!(
            matches!(&s.msgs[0], Msg::FileGroup(g)
                if g.reads[0].ok == Some(true) && g.edits[0].ok == Some(false)),
            "read ok, edit failed"
        );
    }

    #[test]
    fn file_group_line_count_tracks_the_renderer() {
        let read = |id: &str, ok: Option<bool>| ReadItem {
            call_id: id.into(),
            file: format!("{id}.rs"),
            ok,
        };
        let edit = |id: &str, ok: Option<bool>| EditItem {
            call_id: id.into(),
            file: format!("{id}.rs"),
            ok,
        };
        // All settled: folded ok line + one line per failure.
        let group = FileGroup {
            reads: vec![read("a", Some(true)), read("b", Some(false))],
            edits: vec![edit("c", Some(false))],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        assert_eq!(file_group_line_count(&group), 3);
        // Reads pending: breathing line + settled failures.
        let group = FileGroup {
            reads: vec![read("a", None), read("b", Some(false))],
            edits: vec![],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        assert_eq!(file_group_line_count(&group), 2);
        // Edits pending: ok reads line + failed reads + breathing edits + failed edits.
        let group = FileGroup {
            reads: vec![read("a", Some(true)), read("b", Some(false))],
            edits: vec![edit("c", None), edit("d", Some(false))],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        assert_eq!(file_group_line_count(&group), 4);
        // All failed: no folded line, one row per failure.
        let group = FileGroup {
            reads: vec![read("a", Some(false))],
            edits: vec![edit("c", Some(false))],
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
        s.apply_event(&event("tool/call", serde_json::json!({
            "callId": "c1", "name": "bash", "arguments": "{}"
        })));
        assert!(!s.working, "tool call is visible activity");
        s.apply_event(&event("tool/result", serde_json::json!({
            "message": {"content": [{
                "type": "tool-result", "toolCallId": "c1",
                "content": [{"type": "text", "text": "ok"}]
            }]}
        })));
        assert!(s.working, "model works again after a tool result");
        s.apply_event(&event("assistant/chunk", serde_json::json!({
            "chunk": {"type": "text-delta", "text": "hi"}
        })));
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
        s.apply_event(&event("user/message", serde_json::json!({
            "content": [{"type": "text", "text": "hi"}],
            "source": {"kind": "user"}
        })));
        assert!(s.working, "running echo keeps Working alive");
        s.status = AgentStatus::Idle;
        s.apply_event(&event("user/message", serde_json::json!({
            "content": [{"type": "text", "text": "hi"}],
            "source": {"kind": "user"}
        })));
        assert!(!s.working, "idle echo clears Working");
    }

    /// Esc interrupt regression: a turn ending without result events must
    /// settle every still-running indicator so no bullet keeps breathing.
    #[test]
    fn interrupt_settles_running_tool_card() {
        let mut s = AppState::default();
        s.apply_event(&event("turn/start", serde_json::json!({})));
        s.apply_event(&event("tool/call", serde_json::json!({
            "callId": "c1", "name": "bash", "arguments": "{}"
        })));
        assert!(
            matches!(&s.msgs[1], Msg::Tool(card) if card.state == ToolState::Running),
            "tool running before the interrupt"
        );
        s.apply_event(&event("turn/end", serde_json::json!({
            "reason": {"kind": "aborted"}
        })));
        assert!(
            matches!(
                s.msgs.iter().find(|m| matches!(m, Msg::Tool(_))),
                Some(Msg::Tool(card)) if matches!(card.state, ToolState::Done { ok: false, .. })
            ),
            "interrupted tool settles as failed"
        );
        assert!(!s.working);
        assert!(
            s.msgs.iter().any(|m| matches!(m, Msg::System { text } if text == "（已中断）")),
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
        s.apply_event(&event("assistant/chunk", serde_json::json!({
            "chunk": {"type": "text-delta", "text": "部分输出"}
        })));
        assert!(matches!(s.msgs.last(), Some(Msg::Streaming { .. })));
        s.apply_event(&event("turn/end", serde_json::json!({
            "reason": {"kind": "aborted"}
        })));
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
        s.apply_event(&event("tool/call", serde_json::json!({
            "callId": "r1", "name": "read", "arguments": "{\"file\": \"a.txt\"}"
        })));
        s.apply_event(&event("tool/result", serde_json::json!({
            "message": {"content": [{
                "type": "tool-result", "toolCallId": "r1",
                "content": [{"type": "text", "text": "ok"}]
            }]}
        })));
        assert!(
            s.msgs
                .iter()
                .any(|m| matches!(m, Msg::Thinking(card) if card.state == ThinkState::Running)),
            "thinking runs after the read result"
        );
        // Second read is still pending when the interrupt lands.
        s.apply_event(&event("tool/call", serde_json::json!({
            "callId": "r2", "name": "read", "arguments": "{\"file\": \"b.txt\"}"
        })));
        s.apply_event(&event("turn/end", serde_json::json!({
            "reason": {"kind": "aborted"}
        })));
        let group = s.msgs.iter().find_map(|m| match m {
            Msg::FileGroup(group) => Some(group),
            _ => None,
        })
        .expect("file group present");
        assert!(!group.pending(), "interrupted read settles");
        assert!(
            group.reads.iter().any(|i| i.ok == Some(false)),
            "unresolved read marked failed"
        );
        assert!(
            s.msgs.iter().all(|m| !matches!(m, Msg::Thinking(card) if card.state == ThinkState::Running)),
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
        s.apply_event(&event("assistant/chunk", serde_json::json!({
            "chunk": {"type": "text-delta", "text": "hi"}
        })));
        assert!(
            matches!(s.msgs.first(), Some(Msg::Thinking(card)) if card.state == ThinkState::Done),
            "activity settles the Thinking row green"
        );
        // A tool result starts a fresh phase.
        s.apply_event(&event("tool/result", serde_json::json!({
            "message": {"content": [{
                "type": "tool-result", "toolCallId": "c1",
                "content": [{"type": "text", "text": "ok"}]
            }]}
        })));
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
        s.apply_event(&event("tool/call", serde_json::json!({
            "callId": "c1", "name": "read", "arguments": "{\"file\": \"ui.rs\"}"
        })));
        s.apply_event(&event("tool/result", serde_json::json!({
            "message": {"content": [{
                "type": "tool-result", "toolCallId": "c1",
                "content": [{"type": "text", "text": "ok"}]
            }]}
        })));
        let thinking: Vec<&Msg> = s.msgs.iter().filter(|m| matches!(m, Msg::Thinking(_))).collect();
        assert_eq!(thinking.len(), 2, "activity splits the phases: {thinking:?}");
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
        let thinking: Vec<&Msg> = s.msgs.iter().filter(|m| matches!(m, Msg::Thinking(_))).collect();
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
        assert_eq!(s.take_next_queued(), None, "a just-dispatched prompt holds the queue");
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
        s.apply("welcome", &serde_json::json!({"sessionId": "b", "status": "idle"}));
        assert!(s.queue.is_empty(), "queued prompts stay with their session");
    }

    #[test]
    fn welcome_sets_and_switch_clears_the_title() {
        let mut s = AppState::default();
        s.apply("welcome", &serde_json::json!({
            "sessionId": "a", "status": "idle", "title": "第一个标题"
        }));
        assert_eq!(s.session_title.as_deref(), Some("第一个标题"));
        // Switching sessions clears the old title; the new welcome's title
        // (or absence of one) replaces it.
        s.apply("welcome", &serde_json::json!({"sessionId": "b", "status": "idle"}));
        assert_eq!(s.session_title, None, "title follows the session switch");
    }

    #[test]
    fn welcome_sets_cwd_and_switch_clears_it() {
        let mut s = AppState::default();
        s.apply("welcome", &serde_json::json!({
            "sessionId": "a", "status": "idle", "cwd": r"D:\MyProjects\Chore\dsh"
        }));
        assert_eq!(s.session_cwd.as_deref(), Some(r"D:\MyProjects\Chore\dsh"));
        // Switching sessions clears the old path; the new welcome's cwd
        // (or absence of one) replaces it.
        s.apply("welcome", &serde_json::json!({"sessionId": "b", "status": "idle"}));
        assert_eq!(s.session_cwd, None, "cwd follows the session switch");
    }

    #[test]
    fn title_event_updates_the_title_row() {
        let mut s = AppState::default();
        s.apply_event(&event("session/title", serde_json::json!({ "title": "自动生成" })));
        assert_eq!(s.session_title.as_deref(), Some("自动生成"));
        // The title is not part of the transcript cache.
        assert!(s.msgs.is_empty());
    }

    #[test]
    fn snapshot_replay_suppresses_thinking_rows() {
        let mut s = AppState::default();
        s.apply("snapshot", &serde_json::json!({
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
        }));
        assert!(
            !s.msgs.iter().any(|m| matches!(m, Msg::Thinking(_))),
            "history replay has no Thinking rows"
        );
        assert!(!s.working, "replayed turn ended");
    }

    #[test]
    fn snapshot_mid_turn_attaches_live_thinking() {
        let mut s = AppState::default();
        s.apply("snapshot", &serde_json::json!({
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
        }));
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
    fn cache_invalidation_is_incremental() {        let mut s = AppState::default();
        s.cache_valid = true;
        s.tail_dirty = false;
        // Non-rendered events don't touch the cache.
        s.apply_event(&event("step/tool", serde_json::json!({"x": 1})));
        assert!(s.cache_valid, "step events must not invalidate");
        // Structural change invalidates.
        s.apply_event(&event("user/message", serde_json::json!({
            "content": [{"type": "text", "text": "hi"}],
            "source": {"kind": "user"}
        })));
        assert!(!s.cache_valid);
        s.cache_valid = true;
        s.tail_dirty = false;
        // The first chunk creates a Streaming message → structural.
        s.apply_event(&event("assistant/chunk", serde_json::json!({
            "chunk": {"type": "text-delta", "text": "a"}
        })));
        assert!(!s.cache_valid);
        s.cache_valid = true;
        s.tail_dirty = false;
        // Appended chunks only dirty the tail.
        s.apply_event(&event("assistant/chunk", serde_json::json!({
            "chunk": {"type": "text-delta", "text": "b"}
        })));
        assert!(s.cache_valid, "chunk append must not invalidate the cache");
        assert!(s.tail_dirty, "chunk append must mark the tail dirty");
        assert!(matches!(&s.msgs[1], Msg::Streaming { text } if text == "ab"));
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
        assert_eq!(breathing_color(&theme, 0.5), theme.running, "yellow at phase 0.5");
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
        assert!(tick_spinners(&mut s, std::time::Instant::now()), "running drives redraws");
        assert!(s.activity_epoch.is_some(), "epoch set while running");
        assert!(!s.cache_valid, "animation invalidates the cache");
        // Settle: the transition still drives redraws.
        card.state = ToolState::Done { ok: true, lines: 0, duration_ms: 0 };
        card.done_since = Some(std::time::Instant::now());
        card.done_from = Some(Theme::ferra().dim);
        s.msgs[0] = Msg::Tool(card.clone());
        s.cache_valid = true;
        assert!(tick_spinners(&mut s, std::time::Instant::now()), "transition animates");
        // Long-settled: the clock stops.
        card.done_since = Some(
            std::time::Instant::now()
                - std::time::Duration::from_millis(SETTLE_TRANSITION_MS as u64 + 10),
        );
        s.msgs[0] = Msg::Tool(card);
        s.cache_valid = true;
        assert!(!tick_spinners(&mut s, std::time::Instant::now()), "settled card stops redraws");
        assert!(s.cache_valid, "no invalidation when nothing animates");
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
        s.cache_valid = true;
        assert!(tick_spinners(&mut s, std::time::Instant::now()), "running drives redraws");
        assert!(s.activity_epoch.is_some(), "epoch set while running");
        assert!(!s.cache_valid, "animation invalidates the cache");
        // Idle with nothing animating stops the clock again.
        s.status = AgentStatus::Idle;
        assert!(!tick_spinners(&mut s, std::time::Instant::now()), "idle stops redraws");
        assert!(s.activity_epoch.is_none(), "epoch cleared when idle");
    }

    #[test]
    fn snapshot_sets_history_window() {
        let mut s = AppState::default();
        s.apply("snapshot", &serde_json::json!({
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
        }));
        assert_eq!(s.min_seq, Some(5), "earliest seq tracked");
        assert!(!s.history_exhausted, "truncated snapshot has older history");
        assert!(!s.history_loading);

        let mut s2 = AppState::default();
        s2.apply("snapshot", &serde_json::json!({
            "events": [event_seq("user/message", 1, serde_json::json!({
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }))],
            "truncated": false
        }));
        assert!(s2.history_exhausted, "full snapshot = nothing older to load");
    }

    #[test]
    fn prepend_events_stitches_in_front() {
        let mut s = AppState::default();
        s.apply_event(&event_seq("user/message", 10, serde_json::json!({
            "content": [{"type": "text", "text": "后"}],
            "source": {"kind": "user"}
        })));
        s.cache_valid = true;
        s.render_cache = vec![ratatui::text::Line::default(); 4];
        let added = s.prepend_events(&[event_seq("user/message", 2, serde_json::json!({
            "content": [{"type": "text", "text": "前"}],
            "source": {"kind": "user"}
        }))]);
        assert_eq!(added, 1);
        assert_eq!(s.msgs.len(), 2);
        assert!(matches!(&s.msgs[0], Msg::User { text } if text == "前"));
        assert!(matches!(&s.msgs[1], Msg::User { text } if text == "后"));
        assert!(!s.cache_valid, "prepend forces a full rebuild");
        assert_eq!(s.prepend_line_anchor, Some(4));
        assert_eq!(s.min_seq, Some(2));
    }

    #[test]
    fn prepend_drops_dangling_streaming() {
        let mut s = AppState::default();
        s.apply_event(&event_seq("assistant/message", 10, serde_json::json!({
            "message": {"content": [{"type": "text", "text": "全文"}]}
        })));
        let added = s.prepend_events(&[
            event_seq("user/message", 2, serde_json::json!({
                "content": [{"type": "text", "text": "问"}],
                "source": {"kind": "user"}
            })),
            event_seq("assistant/chunk", 8, serde_json::json!({
                "chunk": {"type": "text-delta", "text": "部分"}
            })),
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
        assert_eq!(answers[0].custom, None, "whitespace-only draft drops custom");
    }
}
