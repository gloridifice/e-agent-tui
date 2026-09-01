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
    pub queue: Vec<crate::PromptInput>,
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
            queue: Vec::new(),
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
