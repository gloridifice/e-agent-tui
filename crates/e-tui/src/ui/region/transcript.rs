use ratatui::{layout::Rect, Frame};

use crate::{app::TuiApp, interaction::ScrollState, theme::Theme};

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
) {
    super::super::transcript::render_transcript(frame, area, state, scroll, theme, help_visible);
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
) -> usize {
    super::super::transcript::render_transcript_combined(
        frame,
        area,
        state,
        scroll,
        theme,
        help_visible,
        bottom_stack,
    )
}
