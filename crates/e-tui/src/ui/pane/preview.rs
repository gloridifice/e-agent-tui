use ratatui::{layout::Rect, Frame};

use crate::{preview::PreviewPaneState, theme::Theme};

pub fn render(frame: &mut Frame, area: Rect, preview: &mut PreviewPaneState, theme: &Theme) {
    super::super::region::preview::render(frame, area, preview, theme);
}
