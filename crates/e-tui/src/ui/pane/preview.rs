use ratatui::{layout::Rect, Frame};

use crate::{config::Config, preview::PreviewPaneState, theme::Theme};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    preview: &mut PreviewPaneState,
    config: &Config,
    theme: &Theme,
) {
    super::super::region::preview::render(frame, area, preview, config, theme);
}
