use ratatui::{
    layout::{Position, Rect},
    Frame,
};

use crate::{input::InputState, theme::Theme};

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    input: &InputState,
    theme: &Theme,
    toast: Option<&str>,
    horizontal_padding: u16,
) -> Option<Position> {
    super::super::input::render_input(frame, area, input, theme, toast, horizontal_padding)
}
