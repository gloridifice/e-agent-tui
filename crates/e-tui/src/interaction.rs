//! Kernel-neutral interaction state and blocking-input ownership.

use crate::{
    action::AgentRequest,
    config::{Config, PaneWidthPercent},
    input::InputState,
    input_page::InputPageSession,
    mouse_selection::MouseSelection,
    notice::NoticeState,
};

/// Minimum usable Preview content width at which the split remains useful.
pub const MIN_PREVIEW_COLUMNS: u16 = 16;
/// The split Preview rectangle reserves one separator column, one blank
/// column after it, the usable content width, and one right margin.
pub const PREVIEW_SEPARATOR_COLUMNS: u16 = 1;
pub const PREVIEW_SEPARATOR_GAP_COLUMNS: u16 = 1;
pub const PREVIEW_RIGHT_MARGIN_COLUMNS: u16 = 1;
pub const MIN_PREVIEW_PANE_WIDTH: u16 = PREVIEW_SEPARATOR_COLUMNS
    + PREVIEW_SEPARATOR_GAP_COLUMNS
    + MIN_PREVIEW_COLUMNS
    + PREVIEW_RIGHT_MARGIN_COLUMNS;

/// Transient state for the captured primary-button pane separator gesture.
/// Pending values are presentation-only until `finish` commits the gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaneResizeState {
    drag: Option<PaneResizeDrag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneResizeDrag {
    pub start_column: u16,
    pub started_collapsed: bool,
    pub pending_percent: PaneWidthPercent,
    pub pending_collapsed: bool,
}

impl PaneResizeState {
    pub fn is_active(self) -> bool {
        self.drag.is_some()
    }

    pub fn drag(self) -> Option<PaneResizeDrag> {
        self.drag
    }

    pub fn begin(
        &mut self,
        start_column: u16,
        committed_percent: PaneWidthPercent,
        collapsed: bool,
    ) -> bool {
        if self.drag.is_some() {
            return false;
        }
        self.drag = Some(PaneResizeDrag {
            start_column,
            started_collapsed: collapsed,
            pending_percent: committed_percent,
            pending_collapsed: collapsed,
        });
        true
    }

    /// Update the pending split from a terminal column. The same arithmetic is
    /// used for expanded and collapsed gestures so the placeholder guide and
    /// the eventual committed layout cannot diverge by more than rounding.
    pub fn update(&mut self, column: u16, total_columns: u16) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if total_columns == 0 {
            return;
        }

        let min_main = PaneWidthPercent::from_basis_points(PaneWidthPercent::MIN_BASIS_POINTS)
            .expect("percentage minimum is valid")
            .columns(total_columns);
        let can_split = total_columns >= min_main.saturating_add(MIN_PREVIEW_PANE_WIDTH);

        let (pending_percent, pending_collapsed) = if drag.started_collapsed {
            let leftward = drag.start_column.saturating_sub(column);
            if leftward == 0 || !can_split {
                (drag.pending_percent, true)
            } else {
                let preview_width =
                    MIN_PREVIEW_PANE_WIDTH.saturating_add(leftward.saturating_sub(1));
                if preview_width > total_columns.saturating_sub(min_main) {
                    (drag.pending_percent, true)
                } else {
                    let main_width = total_columns.saturating_sub(preview_width);
                    (
                        PaneWidthPercent::from_columns(main_width, total_columns),
                        false,
                    )
                }
            }
        } else {
            let main_width = column.min(total_columns);
            let percent = PaneWidthPercent::from_columns(main_width, total_columns);
            let rendered_main = percent.columns(total_columns);
            let preview_width = total_columns.saturating_sub(rendered_main);
            if !can_split || preview_width < MIN_PREVIEW_PANE_WIDTH {
                (
                    PaneWidthPercent::from_basis_points(PaneWidthPercent::MAX_BASIS_POINTS)
                        .expect("percentage maximum is valid"),
                    true,
                )
            } else {
                (percent, false)
            }
        };

        drag.pending_percent = pending_percent;
        drag.pending_collapsed = pending_collapsed;
    }

    pub fn finish(&mut self) -> Option<PaneResizeDrag> {
        self.drag.take()
    }

    pub fn cancel(&mut self) {
        self.drag = None;
    }
}

/// One pending approval prompt owned by the frontend interaction lifecycle.
#[derive(Debug, Clone)]
pub struct ApprovalCard {
    pub id: String,
    pub tool_name: String,
    pub reason: String,
}

impl ApprovalCard {
    pub fn answer(self, allow: bool) -> AgentRequest {
        AgentRequest::ApprovalAnswer { id: self.id, allow }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollState {
    pub follow: bool,
    pub offset: usize,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            follow: true,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptDelivery {
    Asap,
    AfterTurn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPrompt {
    pub prompt: crate::PromptInput,
    pub delivery: PromptDelivery,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingPromptQueue {
    local: Vec<PendingPrompt>,
    remote: Vec<PendingPrompt>,
    sending: Option<PendingPrompt>,
    failed: Vec<PendingPrompt>,
    clearing: bool,
    clear_sent: bool,
    display: Vec<PendingPrompt>,
}

impl PendingPromptQueue {
    pub fn push(&mut self, prompt: crate::PromptInput, delivery: PromptDelivery) {
        self.local.push(PendingPrompt { prompt, delivery });
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.display = self
            .remote
            .iter()
            .chain(self.sending.iter())
            .chain(self.failed.iter())
            .chain(self.local.iter())
            .cloned()
            .collect();
    }

    pub fn entries(&self) -> &[PendingPrompt] {
        &self.display
    }

    pub fn len(&self) -> usize {
        self.display.len()
    }

    pub fn is_empty(&self) -> bool {
        self.display.is_empty() && !self.clearing
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn has_asap(&self) -> bool {
        self.clearing
            || self
                .display
                .iter()
                .any(|entry| entry.delivery == PromptDelivery::Asap)
    }

    pub fn has_backend(&self) -> bool {
        self.sending.is_some() || !self.remote.is_empty()
    }

    pub fn cancel_asap(&mut self) {
        if self.clearing {
            return;
        }
        self.local
            .retain(|entry| entry.delivery != PromptDelivery::Asap);
        self.failed.clear();
        self.clearing = self.has_backend();
        self.rebuild();
    }

    pub fn take_clear_request(&mut self) -> bool {
        if self.clearing && !self.clear_sent && self.sending.is_none() {
            self.clear_sent = true;
            true
        } else {
            false
        }
    }

    pub fn complete_clear(&mut self) {
        self.clearing = false;
        self.clear_sent = false;
    }

    pub fn begin_submission(&mut self, pending: PendingPrompt) {
        self.sending = Some(pending);
        self.rebuild();
    }

    pub fn complete_submission(&mut self, failed: bool) {
        if let Some(pending) = self.sending.take() {
            if failed && !self.clearing {
                self.failed.push(pending);
            }
        }
        self.rebuild();
    }

    pub fn update_remote(&mut self, prompts: Vec<String>) {
        self.remote = prompts
            .into_iter()
            .map(|text| PendingPrompt {
                prompt: text.into(),
                delivery: PromptDelivery::Asap,
            })
            .collect();
        self.rebuild();
    }

    pub fn cancel_latest(&mut self) -> Option<PendingPrompt> {
        let pending = self.local.pop();
        self.rebuild();
        pending
    }

    pub fn take_next(&mut self, agent_running: bool) -> Option<PendingPrompt> {
        if self.sending.is_some() || self.clearing {
            return None;
        }
        let index = self
            .local
            .iter()
            .position(|entry| entry.delivery == PromptDelivery::Asap)
            .or_else(|| {
                (!agent_running
                    && self.remote.is_empty()
                    && self.failed.is_empty()
                    && !self.local.is_empty())
                .then_some(0)
            })?;
        let pending = self.local.remove(index);
        self.rebuild();
        Some(pending)
    }
}

/// State whose lifetime follows local user interaction rather than a wire
/// message family. It is the only production owner for composer, focus,
/// blocking pages, queue, and legacy copy-navigation state.
pub struct InteractionModel {
    pub input: InputState,
    pub scroll: ScrollState,
    pub input_page: Option<InputPageSession>,
    pub help_visible: bool,
    pub approval: Option<ApprovalCard>,
    pub question: Option<String>,
    pub queue: PendingPromptQueue,
    pub notice: NoticeState,
    pub mouse_selection: MouseSelection,
    pub pane_resize: PaneResizeState,
}

impl InteractionModel {
    pub fn new(config: &Config) -> Self {
        Self {
            input: InputState::new(config),
            scroll: ScrollState::default(),
            input_page: None,
            help_visible: false,
            approval: None,
            question: None,
            queue: PendingPromptQueue::default(),
            notice: NoticeState::default(),
            mouse_selection: MouseSelection::default(),
            pane_resize: PaneResizeState::default(),
        }
    }
}

impl Default for InteractionModel {
    fn default() -> Self {
        Self::new(&Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_queue_prioritizes_asap_and_cancels_them_as_a_batch() {
        let mut dispatch = PendingPromptQueue::default();
        for (text, delivery) in [
            ("a", PromptDelivery::Asap),
            ("b", PromptDelivery::AfterTurn),
            ("c", PromptDelivery::Asap),
            ("d", PromptDelivery::AfterTurn),
        ] {
            dispatch.push(text.into(), delivery);
        }
        assert_eq!(dispatch.take_next(true).unwrap().prompt, "a");
        assert_eq!(dispatch.take_next(true).unwrap().prompt, "c");
        assert!(dispatch.take_next(true).is_none());
        assert_eq!(dispatch.take_next(false).unwrap().prompt, "b");
        assert_eq!(dispatch.take_next(false).unwrap().prompt, "d");

        let mut cancellation = PendingPromptQueue::default();
        for (text, delivery) in [
            ("a", PromptDelivery::Asap),
            ("b", PromptDelivery::AfterTurn),
            ("c", PromptDelivery::Asap),
            ("d", PromptDelivery::AfterTurn),
        ] {
            cancellation.push(text.into(), delivery);
        }
        cancellation.cancel_asap();
        let cancelled = std::iter::from_fn(|| cancellation.cancel_latest())
            .map(|entry| entry.prompt.plain_text().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(cancelled, ["d", "b"]);
    }

    #[test]
    fn cancellation_waits_for_admission_and_preserves_new_submissions() {
        let mut queue = PendingPromptQueue::default();
        queue.push("a".into(), PromptDelivery::Asap);
        queue.push("b".into(), PromptDelivery::AfterTurn);
        let sending = queue.take_next(true).unwrap();
        queue.begin_submission(sending);
        queue.push("c".into(), PromptDelivery::Asap);
        queue.cancel_asap();
        assert!(!queue.take_clear_request());
        assert!(queue.take_next(true).is_none());
        queue.push("new".into(), PromptDelivery::Asap);
        queue.cancel_asap();
        queue.update_remote(vec!["expanded a".into(), "extension".into()]);
        queue.complete_submission(false);
        assert!(queue.take_clear_request());
        assert!(!queue.take_clear_request());
        assert!(queue.take_next(true).is_none());
        queue.update_remote(Vec::new());
        assert!(!queue.is_empty());
        queue.complete_clear();
        assert_eq!(queue.take_next(true).unwrap().prompt, "new");
        assert_eq!(queue.cancel_latest().unwrap().prompt, "b");
        assert!(queue.is_empty());
    }

    #[test]
    fn duplicate_remote_prompts_are_authoritative_not_text_matched() {
        let mut queue = PendingPromptQueue::default();
        queue.update_remote(vec!["same".into(), "same".into()]);
        queue.push("same".into(), PromptDelivery::AfterTurn);
        assert_eq!(queue.len(), 3);
        queue.update_remote(vec!["same".into()]);
        assert_eq!(queue.len(), 2);
        assert!(queue.take_next(false).is_none());
        queue.cancel_asap();
        assert!(queue.take_clear_request());
        queue.complete_clear(); // Failed clear: snapshot was not emptied.
        assert!(queue.has_asap());
        assert_eq!(queue.len(), 2);
        queue.cancel_asap();
        assert!(queue.take_clear_request());
        queue.clear();
        assert!(queue.is_empty());
        assert!(!queue.take_clear_request());
    }

    #[test]
    fn rejected_admission_retains_payload_without_auto_retry() {
        let mut queue = PendingPromptQueue::default();
        queue.push("rejected".into(), PromptDelivery::Asap);
        let sending = queue.take_next(true).unwrap();
        queue.begin_submission(sending);
        queue.complete_submission(true);
        assert_eq!(queue.entries()[0].prompt, "rejected");
        assert!(queue.take_next(false).is_none());
        queue.cancel_asap();
        assert!(queue.is_empty());
    }

    #[test]
    fn expanded_drag_clamps_the_message_minimum_and_collapses_preview() {
        let mut resize = PaneResizeState::default();
        assert!(resize.begin(60, PaneWidthPercent::default(), false));
        resize.update(10, 100);
        let drag = resize.drag().unwrap();
        assert_eq!(drag.pending_percent.basis_points(), 2_500);
        assert!(!drag.pending_collapsed);

        resize.update(99, 100);
        let drag = resize.drag().unwrap();
        assert_eq!(drag.pending_percent.basis_points(), 10_000);
        assert!(drag.pending_collapsed);
    }

    #[test]
    fn collapsed_drag_restores_at_sixteen_content_columns_and_continues_left() {
        let mut resize = PaneResizeState::default();
        resize.begin(98, PaneWidthPercent::default(), true);
        resize.update(97, 100);
        let drag = resize.drag().unwrap();
        assert!(!drag.pending_collapsed);
        assert_eq!(drag.pending_percent.columns(100), 81);

        resize.update(80, 100);
        let drag = resize.drag().unwrap();
        assert!(!drag.pending_collapsed);
        assert_eq!(drag.pending_percent.columns(100), 64);
    }

    #[test]
    fn impossible_small_width_stays_collapsed_without_violating_minimum() {
        let mut resize = PaneResizeState::default();
        resize.begin(12, PaneWidthPercent::default(), false);
        resize.update(5, 20);
        let drag = resize.drag().unwrap();
        assert!(drag.pending_collapsed);
        assert_eq!(drag.pending_percent.basis_points(), 10_000);
    }

    #[test]
    fn cancellation_discards_pending_resize() {
        let mut resize = PaneResizeState::default();
        resize.begin(60, PaneWidthPercent::default(), false);
        resize.update(40, 100);
        resize.cancel();
        assert!(!resize.is_active());
    }
}
