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
    breathing_color, lerp_color, settle_color, ACTIVITY_SPINNER_FRAMES, BREATH_CYCLE_MS,
    SETTLE_TRANSITION_MS,
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
use crate::{
    agent::timeline::{SurfaceOperation, TimelineFact, TimelineRecord, TokenUsage},
    preview::{
        MutationDiff, MutationHunk, ToolMetrics, ToolPreview, ToolPreviewPrimary,
        ToolPreviewSecondary,
    },
    PreviewContent, PreviewKey, PreviewRef, PreviewRevision, TuiApp,
};

#[cfg(test)]
mod legacy;
#[cfg(test)]
use legacy::*;

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

    /// The model started thinking (or the user sent a message before the
    /// turn exists): show a yellow Braille `Thinking...` spinner. Only
    /// ADJACENT thinking phases collapse into one row with a count (`xN`) — any
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

    /// The thinking phase ended (visible activity took over, the turn
    /// ended, or the agent went idle): settle the running Thinking row.
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
            if self.settle_activity(&id, kind != "error", text) {
                self.reconcile_latest_preview();
            }
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

    pub fn is_agent_idle(&self) -> bool {
        self.session.status == AgentStatus::Idle && !self.session.working
    }

    pub fn is_fully_idle(&self) -> bool {
        self.is_agent_idle() && !self.has_active_command()
    }

    fn has_active_turn(&self) -> bool {
        self.session.status == AgentStatus::Running
    }

    /// Queue a prompt typed while work is active. As-soon-as-possible prompts
    /// are eligible for steering during the current turn; after-turn prompts
    /// remain local until the agent and any command are fully idle.
    pub fn enqueue_or_immediate(
        &mut self,
        prompt: crate::PromptInput,
        delivery: crate::interaction::PromptDelivery,
        queue: &mut crate::interaction::PendingPromptQueue,
    ) -> bool {
        if queue.is_empty()
            && self.session.temporary_model.is_none()
            && (self.is_fully_idle()
                || (delivery == crate::interaction::PromptDelivery::Asap && self.is_agent_idle()))
        {
            true
        } else {
            queue.push(prompt, delivery);
            false
        }
    }

    /// Claim one queued prompt. Steering prompts always outrank after-turn
    /// prompts; the latter are eligible only after all work has settled.
    pub fn take_next_queued(&mut self) -> Option<crate::interaction::PendingPrompt> {
        let active_turn = self.has_active_turn();
        if self.interaction.queue.is_empty() || (!active_turn && !self.is_agent_idle()) {
            return None;
        }
        let asap_only = !self.is_fully_idle();
        self.interaction.queue.take_next(asap_only)
    }

    /// Settle everything still running when a turn ends: interrupted turns
    /// leave tool cards, file-group items, and Thinking rows without their
    /// result events — without this their Braille spinners would run forever
    /// (the Esc-interrupt bug). A dangling streaming tail is finalized into a
    /// regular assistant message so its spinner also stops.
    pub fn settle_turn(&mut self, now_ms: u64, cancelled: bool) {
        let active_ids = self
            .transcript
            .nodes()
            .iter()
            .filter_map(|node| match &node.item {
                DisplayItem::Activity(row)
                    if row.state.is_active() && !row.id.0.starts_with("retry-progress:") =>
                {
                    Some(row.id.clone())
                }
                DisplayItem::Block(block) if block.streaming => Some(block.id.clone()),
                DisplayItem::Thinking(node) if node.row.state.is_active() || node.streaming => {
                    Some(node.row.id.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for id in active_ids {
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
                    Msg::Activity(row)
                        if row.state.is_active() && !row.id.0.starts_with("retry-progress:") =>
                    {
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
        self.render.activity_frame = 0;
        self.preview.clear();
        self.clear_reading_layout_anchor();
        self.reading = None;
        self.reading_document = crate::ReadingDocument::default();
        self.reading_layout = crate::ReadingLayout::default();
        #[cfg(test)]
        self.msgs.clear();
        self.projector = EventProjector::default();
        self.todos.clear();
        self.goal = None;
        self.plan_mode = None;
        self.session.current_mode = None;
        self.session.current_mode_seq = None;
        self.session.token_usage = TokenUsage::default();
        self.session.cost_usd = None;
        self.session.last_usage_sample = None;
        self.session.context_usage_unknown = false;
        self.link_copy.clear();
        self.session_state_events.clear();
        self.render.units.clear();
        self.render.next_unit = 0;
        self.next_thinking_id = 0;
        self.next_local_display_id = 0;
        self.pending_transcript_insert = None;
        self.pending_submissions.clear();
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

    #[test]
    fn compaction_feedback_waits_for_fresh_usage_and_survives_history() {
        let mut state = RuntimeState::default();
        let usage = TokenUsage {
            input_tokens: 400,
            ..Default::default()
        };
        state.session.record_usage(Some(2), Some(1), Some(usage));
        let start = record(
            10,
            TimelineFact::CompactionStarted {
                id: "c".into(),
                model_name: Some("Small".into()),
            },
        );
        let finish = record(
            11,
            TimelineFact::CompactionFinished {
                id: "c".into(),
                model_name: Some("Small".into()),
                error: None,
            },
        );
        state.apply_host_event(&start);
        let id = DisplayId::correlated("compaction", "c");
        let DisplayItem::Activity(running) = &state.transcript.get(&id).unwrap().item else {
            panic!("activity")
        };
        assert_eq!(running.label, "compacting with Small");
        state.apply_host_event(&finish);
        let DisplayItem::Activity(row) = &state.transcript.get(&id).unwrap().item else {
            panic!("activity")
        };
        assert_eq!(row.label, "compacting complete with Small");
        assert_eq!(row.state, ActivityState::Success);
        assert!(state.session.context_usage_unknown);
        assert_eq!(state.session.token_usage, usage);
        state.session.record_usage(Some(3), Some(1), None);
        state
            .session
            .record_usage(Some(3), Some(1), Some(TokenUsage::default()));
        assert!(state.session.context_usage_unknown);
        state.prepend_host_events(&[record(
            1,
            TimelineFact::AssistantMessage {
                text: "old".into(),
                reasoning: String::new(),
                content: Vec::new(),
                turn: Some(1),
                step: Some(1),
                usage: Some(usage),
            },
        )]);
        assert!(state.session.context_usage_unknown);
        state.session.record_usage(
            Some(3),
            Some(1),
            Some(TokenUsage {
                input_tokens: 60,
                ..Default::default()
            }),
        );
        assert!(!state.session.context_usage_unknown);
        assert_eq!(state.session.context_usage_percent(1_000), 6);
        state.apply_host_event(&record(
            12,
            TimelineFact::CompactionFinished {
                id: "failed".into(),
                model_name: None,
                error: Some("failed".into()),
            },
        ));
        assert!(!state.session.context_usage_unknown);
    }

    #[test]
    fn compaction_feedback_settlement_before_start_preserves_completed_label() {
        let mut state = RuntimeState::default();
        state.apply_host_event(&record(
            2,
            TimelineFact::CompactionFinished {
                id: "c".into(),
                model_name: Some("Small".into()),
                error: None,
            },
        ));
        state.prepend_host_events(&[record(
            1,
            TimelineFact::CompactionStarted {
                id: "c".into(),
                model_name: Some("Small".into()),
            },
        )]);
        let DisplayItem::Activity(row) = &state
            .transcript
            .get(&DisplayId::correlated("compaction", "c"))
            .unwrap()
            .item
        else {
            panic!("activity")
        };
        assert_eq!(row.label, "compacting complete with Small");
        assert_eq!(row.state, ActivityState::Success);
        assert!(state.session.context_usage_unknown);
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
    fn session_reset_clears_preview_identity_and_freshness() {
        let mut state = RuntimeState::default();
        let target = crate::preview::PreviewTarget {
            id: "shared-id".into(),
            reference: PreviewRef::Deferred {
                key: PreviewKey("shared-key".into()),
                revision: PreviewRevision(1),
            },
        };
        let request = state.preview.select(Some(target.clone())).unwrap();
        state.preview.complete(
            request.request_id,
            request.key,
            request.revision,
            Ok(PreviewContent::Reasoning("old session".into())),
        );
        assert_eq!(
            state.preview.reveal_intent,
            crate::PreviewRevealIntent::FreshLive
        );

        state.reset_transcript();

        assert!(state.preview.target.is_none());
        assert_eq!(state.preview.state, crate::PreviewState::Empty);
        assert!(state.preview.select(Some(target)).is_some());
        assert_eq!(
            state.preview.reveal_intent,
            crate::PreviewRevealIntent::FreshLive
        );
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
    fn skill_read_path_survives_settlement_and_reordered_results() {
        for (path, cwd, expected) in [
            (
                r"G:\repo\skills\review\SKILL.md",
                r"G:\repo",
                "skills/review/SKILL.md",
            ),
            (
                r"C:\skills\review\SKILL.md",
                r"G:\repo",
                "C:/skills/review/SKILL.md",
            ),
            ("SKILL.md", "/skills/review", "SKILL.md"),
        ] {
            for result_first in [false, true] {
                let mut state = RuntimeState::default();
                state.session.session_cwd = Some(cwd.into());
                let mut activity = edit_call();
                activity.capability = crate::agent::tool::ToolCapability::SkillRead;
                activity.summary = "review".into();
                activity.reference =
                    Some(crate::agent::tool::ToolReference::Path { path: path.into() });
                let call = record(1, TimelineFact::ToolCall(activity));
                let result = record(
                    2,
                    TimelineFact::ToolResult {
                        activity_id: "edit-1".into(),
                        output: "unavailable".into(),
                        state: AgentActivityState::Failure,
                        output_truncated: false,
                        execution_metrics: None,
                        starts_thinking: false,
                        mutation_diff: None,
                        mutation_hunks: Vec::new(),
                    },
                );
                for event in if result_first {
                    [&result, &call]
                } else {
                    [&call, &result]
                } {
                    state.apply_host_event(event);
                }
                let node = state
                    .transcript
                    .get(&crate::display::DisplayId::correlated(
                        "tool-call",
                        "edit-1",
                    ))
                    .unwrap();
                let crate::display::DisplayItem::Activity(row) = &node.item else {
                    panic!()
                };
                assert_eq!(row.kind, crate::display::ActivityKind::Skill);
                assert_eq!(row.state, crate::display::ActivityState::Failure);
                assert_eq!(row.label, "read");
                assert_eq!(row.summary, "review");
                assert_eq!(row.continuations.len(), 1);
                assert_eq!(row.continuations[0].label, "at");
                assert_eq!(row.continuations[0].summary, expected);
            }
        }
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
                execution_metrics: None,
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
    fn direct_command_result_refreshes_the_same_preview_target() {
        let mut state = RuntimeState::default();
        state.apply_host_event(&record(
            1,
            TimelineFact::CommandStarted {
                id: "build-1".into(),
                name: "build".into(),
                args: Some("--release".into()),
            },
        ));
        let before = state
            .preview
            .target
            .clone()
            .expect("command preview target");
        let before_revision = before.reference.revision();

        state.apply_command_result("build-1", "success", Some("completed"));

        let after = state
            .preview
            .target
            .as_ref()
            .expect("settled preview target");
        assert_eq!(after.id, before.id);
        assert_eq!(after.reference.key(), before.reference.key());
        assert_ne!(after.reference.revision(), before_revision);
        assert!(matches!(
            &state.preview.state,
            crate::PreviewState::Ready(PreviewContent::PlainText(text))
                if text == "/build completed"
        ));
    }

    #[test]
    fn historical_tool_metrics_do_not_derive_duration_from_replay_time() {
        let mut state = RuntimeState::default();
        state.config.read_merge = false;
        state.apply_host_event(&record(1, TimelineFact::ToolCall(edit_call())));
        state.apply_host_event(&record(
            2,
            TimelineFact::ToolResult {
                activity_id: "edit-1".into(),
                output: "native output is not a metric source".into(),
                state: AgentActivityState::Success,
                output_truncated: false,
                execution_metrics: Some(crate::agent::timeline::ToolExecutionMetrics {
                    duration_ms: Some(777),
                    output_lines: Some(9),
                    output_lines_truncated: true,
                    started_unix_ms: Some(100),
                    ended_unix_ms: Some(877),
                }),
                starts_thinking: false,
                mutation_diff: None,
                mutation_hunks: Vec::new(),
            },
        ));
        let row = state
            .transcript
            .nodes()
            .iter()
            .find_map(|node| match &node.item {
                DisplayItem::Activity(row) => Some(row),
                _ => None,
            })
            .unwrap();
        assert_eq!(row.duration_ms, Some(777));
        assert_eq!(row.output_lines, Some(9));
        assert!(row.output_lines_truncated);

        let mut missing = RuntimeState::default();
        missing.config.read_merge = false;
        missing.apply_host_event(&record(1, TimelineFact::ToolCall(edit_call())));
        missing.apply_host_event(&record(
            2,
            TimelineFact::ToolResult {
                activity_id: "edit-1".into(),
                output: "one\ntwo".into(),
                state: AgentActivityState::Success,
                output_truncated: false,
                execution_metrics: Some(crate::agent::timeline::ToolExecutionMetrics {
                    duration_ms: None,
                    output_lines: Some(2),
                    output_lines_truncated: false,
                    started_unix_ms: None,
                    ended_unix_ms: None,
                }),
                starts_thinking: false,
                mutation_diff: None,
                mutation_hunks: Vec::new(),
            },
        ));
        let row = missing
            .transcript
            .nodes()
            .iter()
            .find_map(|node| match &node.item {
                DisplayItem::Activity(row) => Some(row),
                _ => None,
            })
            .unwrap();
        assert_eq!(row.duration_ms, None);
        assert_eq!(row.output_lines, Some(2));

        let mut split = RuntimeState::default();
        split.config.read_merge = false;
        split.apply_host_event(&record(
            2,
            TimelineFact::ToolResult {
                activity_id: "edit-1".into(),
                output: "ignored".into(),
                state: AgentActivityState::Success,
                output_truncated: false,
                execution_metrics: Some(crate::agent::timeline::ToolExecutionMetrics {
                    duration_ms: Some(333),
                    output_lines: Some(4),
                    output_lines_truncated: false,
                    started_unix_ms: Some(100),
                    ended_unix_ms: Some(433),
                }),
                starts_thinking: false,
                mutation_diff: None,
                mutation_hunks: Vec::new(),
            },
        ));
        split.apply_host_event(&record(1, TimelineFact::ToolCall(edit_call())));
        let row = split
            .transcript
            .nodes()
            .iter()
            .find_map(|node| match &node.item {
                DisplayItem::Activity(row) => Some(row),
                _ => None,
            })
            .unwrap();
        assert_eq!(row.duration_ms, Some(333));
        assert_eq!(row.output_lines, Some(4));
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
                execution_metrics: None,
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
