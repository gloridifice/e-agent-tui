use ratatui::{
    layout::{Position, Rect},
    Frame,
};

use crate::{config::InputStyle, input::InputState, theme::Theme};

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    input: &InputState,
    theme: &Theme,
    horizontal_padding: u16,
    input_style: InputStyle,
) -> Option<Position> {
    super::super::input::render_input(frame, area, input, theme, horizontal_padding, input_style)
}
