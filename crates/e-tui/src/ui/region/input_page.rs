use ratatui::{
    layout::{Position, Rect},
    Frame,
};

use crate::{config::Config, input_page::InputPageSession, theme::Theme};

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    page: &mut InputPageSession,
    config: &Config,
    theme: &Theme,
) -> Option<Position> {
    super::super::pages::render_input_page(frame, area, page, config, theme)
}
