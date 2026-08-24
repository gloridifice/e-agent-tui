use ratatui::{layout::Rect, Frame};

use crate::{
    config::Config, mouse_selection::SelectionFrame, preview::PreviewPaneState, theme::Theme,
};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    preview: &mut PreviewPaneState,
    config: &Config,
    theme: &Theme,
    selection_frame: &mut SelectionFrame,
) {
    super::super::region::preview::render(frame, area, preview, config, theme, selection_frame);
}
