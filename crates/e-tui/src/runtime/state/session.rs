//! Session attach, deferred `/new` draft, and queued-prompt state.

use super::*;

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
}
