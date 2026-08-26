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
    left_padding: u16,
    right_padding: u16,
) {
    super::super::region::preview::render(
        frame,
        area,
        preview,
        config,
        theme,
        selection_frame,
        left_padding,
        right_padding,
    );
}
