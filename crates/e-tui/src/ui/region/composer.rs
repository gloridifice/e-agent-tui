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
    horizontal_padding: u16,
    model_hint: Option<&str>,
) -> Option<Position> {
    super::super::input::render_input(frame, area, input, theme, horizontal_padding, model_hint)
}
