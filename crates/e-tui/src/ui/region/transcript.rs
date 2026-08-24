use ratatui::{layout::Rect, Frame};

use crate::{app::TuiApp, interaction::ScrollState, mouse_selection::SelectionFrame, theme::Theme};

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    selection_frame: &mut SelectionFrame,
) {
    super::super::transcript::render_transcript(
        frame,
        area,
        state,
        scroll,
        theme,
        help_visible,
        selection_frame,
    );
}

#[allow(clippy::too_many_arguments)] // Region forwards shared layout inputs unchanged.
pub(crate) fn render_combined(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    bottom_stack: usize,
    selection_frame: &mut SelectionFrame,
) -> usize {
    super::super::transcript::render_transcript_combined(
        frame,
        area,
        state,
        scroll,
        theme,
        help_visible,
        bottom_stack,
        selection_frame,
    )
}
