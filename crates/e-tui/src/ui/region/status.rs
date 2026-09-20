use ratatui::{layout::Rect, Frame};

use crate::{app::TuiApp, interaction::ScrollState, theme::Theme};

pub(crate) fn render_input_header(frame: &mut Frame, area: Rect, state: &TuiApp, theme: &Theme) {
    super::super::status::render_input_header(frame, area, state, theme);
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    state: &TuiApp,
    scroll: &ScrollState,
    theme: &Theme,
) {
    super::super::status::render_status(frame, area, state, scroll, theme);
}

pub(crate) fn render_title(frame: &mut Frame, area: Rect, state: &TuiApp, theme: &Theme) {
    super::super::status::render_title(frame, area, state, theme);
}
