//! Full-screen execution-history pane.

use ratatui::{layout::Rect, Frame};

use crate::{config::Config, history_page::HistoryPage, theme::Theme};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    page: &mut HistoryPage,
    config: &Config,
    theme: &Theme,
) {
    super::super::region::history::render(frame, area, page, &config.key_mapping, theme);
}
