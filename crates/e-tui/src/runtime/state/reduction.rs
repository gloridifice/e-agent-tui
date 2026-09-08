//! Timeline/snapshot/history reduction and assistant/tool/activity mutation application.

#[cfg(test)]
use super::{
    breathing_color, exit_marker, legacy_file_action, settle_group, DisplayTone, FileGroup,
    FileItem, Msg, ThinkState, ThinkingCard, ToolCard, ToolState,
};
use super::{
    host_event_time, is_surface_node, mutation_id, relativize_tool_preview, tool_preview_result,
    ActivityMutation, ActivityRow, ActivityState, AgentStatus, AssistantMutation, CardRole,
    CommandProjection, DisplayId, DisplayItem, LifecycleProjection, PendingActivityEnrichment,
    PendingActivityResult, PendingToolResult, PreviewContent, PreviewKey, PreviewRef,
    PreviewRevision, RuntimeState, ThinkingNode, TimelineFact, TimelineRecord, ToolMetrics,
    ToolMutation, TranscriptBlock, WorkflowProjection,
};
use crate::{
    execution_history::ObservedOutputLines,
    projection::{command, lifecycle, retry, workflow},
};

impl RuntimeState {
    pub(super) fn allocate_copy_unit(&mut self, source: &str) -> u64 {
        let unit = self.render.next_unit;
        self.render.next_unit += 1;
        self.render.units.insert(unit, source.to_owned());
        unit
    }

    pub(super) fn insert_transcript_item(
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

    pub(super) fn upsert_activity(&mut self, mut row: crate::display::ActivityRow) {
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

    pub(super) fn apply_pending_activity_enrichments(&mut self) {
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

    pub(super) fn settle_activity_state(
        &mut self,
        id: &DisplayId,
        state: ActivityState,
        summary: Option<&str>,
    ) -> bool {
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

    pub(super) fn settle_activity(
        &mut self,
        id: &DisplayId,
        success: bool,
        summary: Option<&str>,
    ) -> bool {
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

    pub(super) fn settle_activity_state_or_remember(
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
    pub(super) fn apply_tool_result_to_display(
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
                    lines: ObservedOutputLines::from_output(output, output_truncated).count,
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

    pub(super) fn settle_retry_activities(&mut self) {
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
    pub(super) fn upsert_reasoning(&mut self, text: &str, streaming: bool) {
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
    pub(super) fn reduce_assistant_event(
        &mut self,
        event: &TimelineRecord,
        mutations: Vec<AssistantMutation>,
    ) {
        match &event.fact {
            TimelineFact::UserMessage { .. } => {
                self.projector.tool_family.close_group();
                self.render.transcript_cache.invalidate();
                if self.session.status == AgentStatus::Idle && self.pending_submissions.is_empty() {
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

    pub(super) fn append_assistant_item(&mut self, event: &TimelineRecord, mut item: DisplayItem) {
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
        let echoed = if self.replaying {
            None
        } else if let DisplayItem::Card(card) = &item {
            self.pending_submissions.iter().position(|id| {
                self.transcript.get(id).is_some_and(|node| {
                    matches!(&node.item, DisplayItem::Card(pending)
                        if (pending.role == card.role && pending.content == card.content)
                            || (pending.role == CardRole::Attachment && card.role == CardRole::Attachment)
                            || (pending.role == CardRole::Skill && card.role == CardRole::User
                                && pending.copy_source == card.content))
                })
            })
        } else {
            None
        };
        let echoed_position = echoed.and_then(|index| {
            let id = self.pending_submissions.remove(index);
            let position = self
                .transcript
                .nodes()
                .iter()
                .position(|node| node.id() == &id)?;
            if let Some(node) = self.transcript.remove(position) {
                if let DisplayItem::Card(card) = node.item {
                    if let Some(unit) = card.unit {
                        self.render.units.remove(&unit);
                    }
                }
            }
            Some(position)
        });
        let preferred = echoed_position.or_else(|| {
            matches!(&item, DisplayItem::Card(card)
                if matches!(card.role, CardRole::User | CardRole::Skill | CardRole::Attachment))
            .then(|| {
                self.transcript.nodes().last().and_then(|node| {
                    matches!(&node.item, DisplayItem::Thinking(node)
                        if node.row.id.0.starts_with("thinking:")
                            && node.row.state.is_active())
                    .then(|| self.transcript.len().saturating_sub(1))
                })
            })
            .flatten()
        });
        self.insert_transcript_item(item.clone(), surface_seq, preferred);

        // Injected context previews as complete muted Markdown, including the
        // full instructions hidden behind a compact skill invocation row.
        if let DisplayItem::Card(card) = &item {
            if matches!(card.role, CardRole::Skill | CardRole::Context) {
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

    pub(super) fn upsert_reasoning_store(&mut self, incoming: TranscriptBlock, turn: Option<u64>) {
        // Reasoning chunks accumulate into the merged Thinking+Reasoning
        // node. The target node is the trailing one whose turn matches (live
        // nodes created by `start_thinking` carry `turn: None` and adopt the
        // first chunk's turn). When no node exists for this turn — history
        // replay suppresses the animated indicator — a settled one is
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
                crate::i18n::tr(self.config.language, "transcript.thinking"),
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

    pub(super) fn upsert_answer_store(
        &mut self,
        event: &TimelineRecord,
        incoming: TranscriptBlock,
    ) {
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
        let options = crate::render::transcript_options(
            &self.config,
            &self.render.expanded,
            self.render.transcript_cache.width,
        );
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

    pub(super) fn project_tool_family(&mut self, event: &TimelineRecord) -> Option<ToolMutation> {
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
            TimelineFact::ToolResult {
                starts_thinking, ..
            } => {
                if *starts_thinking {
                    self.start_thinking();
                }
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

    /// Stage a result that arrived before its call: keep it in the projector
    /// until the matching call event replays, so history pages can reorder.
    fn remember_missing_tool_result(&mut self, event: &TimelineRecord, now_ms: u64) {
        if let TimelineFact::ToolResult {
            activity_id,
            output,
            state,
            output_truncated,
            execution_metrics,
            mutation_diff,
            mutation_hunks,
            ..
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
                    execution_metrics: *execution_metrics,
                    time_ms: now_ms,
                    surface_seq: event.sequence,
                    mutation_diff: mutation_diff.clone(),
                    mutation_hunks: mutation_hunks.clone(),
                },
            );
        }
    }

    /// Maintain projector indexes and structured Preview seeds for one tool
    /// call/result and stage inline preview refs for the row.
    fn project_tool_indexes_and_preview(&mut self, event: &TimelineRecord, row: &ActivityRow) {
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
                let lines = ObservedOutputLines::from_output(output, *output_truncated);
                let metrics = ToolMetrics {
                    output_lines: row.output_lines.unwrap_or(lines.count),
                    truncated: lines.truncated || row.output_lines_truncated,
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
    }

    /// Merge a staged result-before-call into the live row: settle state,
    /// metrics, and the finalized structured preview from the same facts.
    fn settle_staged_tool_result(
        &mut self,
        event: &TimelineRecord,
        row: &mut ActivityRow,
        row_id: &DisplayId,
    ) -> Option<PendingToolResult> {
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
                if let Some(metrics) = pending.execution_metrics {
                    row.duration_ms = metrics.duration_ms;
                    row.output_lines = metrics.output_lines;
                    row.output_lines_truncated = metrics.output_lines_truncated;
                } else {
                    row.duration_ms =
                        Some(pending.time_ms.saturating_sub(row.start_ms.unwrap_or(0)));
                    let lines =
                        ObservedOutputLines::from_output(&pending.output, pending.output_truncated);
                    row.output_lines = Some(lines.count);
                    row.output_lines_truncated = lines.truncated;
                }
                row.live_duration_since = None;
            }
        }
        // A call that carried a staged result (result-before-call history)
        // finalizes its structured preview from the same settled facts as the
        // live result path.
        if let (TimelineFact::ToolCall(activity), Some(pending)) = (&event.fact, &pending_result) {
            let seed = self.projector.tool_preview_seeds.get(&activity.id).cloned();
            let lines = ObservedOutputLines::from_output(&pending.output, pending.output_truncated);
            let metrics = ToolMetrics {
                output_lines: row.output_lines.unwrap_or(lines.count),
                truncated: lines.truncated || row.output_lines_truncated,
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
        pending_result
    }

    /// Commit the settled activity row into the transcript and mirror the
    /// result into the legacy fixture.
    fn commit_tool_activity_row(
        &mut self,
        event: &TimelineRecord,
        row: ActivityRow,
        pending_result: Option<PendingToolResult>,
        _now_ms: u64,
    ) {
        let row_id = row.id.clone();
        let surface_seq = event.sequence.filter(|_| is_surface_node(&event.fact));
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
                _now_ms,
            );
        }
    }

    pub(super) fn reduce_tool_family(&mut self, event: &TimelineRecord, mutation: ToolMutation) {
        let now_ms = host_event_time(event);
        let mut row = match mutation {
            ToolMutation::Upsert(row) => row,
            ToolMutation::Ignore => return,
            ToolMutation::MissingResult => {
                self.remember_missing_tool_result(event, now_ms);
                return;
            }
        };
        match &event.fact {
            TimelineFact::ToolCall(_) => {
                self.render.transcript_cache.invalidate();
                self.stop_thinking();
            }
            TimelineFact::ToolResult { .. } => {}
            _ => {}
        }

        let row_id = row.id.clone();
        self.project_tool_indexes_and_preview(event, &row);
        let pending_result = self.settle_staged_tool_result(event, &mut row, &row_id);
        self.commit_tool_activity_row(event, row, pending_result, now_ms);
    }

    #[cfg(test)]
    pub(super) fn upsert_tool_legacy_mirror(&mut self, event: &TimelineRecord, row: &ActivityRow) {
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

    pub(super) fn reduce_activity_families(&mut self, event: &TimelineRecord) -> bool {
        if let Some(projection) = lifecycle::project(event, self.config.language) {
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

    pub(super) fn apply_activity_mutation(&mut self, mutation: ActivityMutation) {
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

    pub(super) fn apply_lifecycle_projection(
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
                        let max_tokens = matches!(
                            &event.fact,
                            TimelineFact::TurnEnd { reason, .. }
                                if reason.as_deref() == Some("max-tokens")
                        );
                        if block.tone == DisplayTone::Error {
                            self.msgs.push(Msg::Error {
                                text: block.content,
                            });
                        } else if max_tokens {
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
