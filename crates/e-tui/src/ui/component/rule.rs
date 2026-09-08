use ratatui::{
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    Frame,
};

use crate::theme::Theme;

pub fn line(width: usize, theme: &Theme) -> Line<'static> {
    if width <= 4 {
        return Line::from(Span::styled(
            "─".repeat(width),
            Style::default().fg(theme.diff.separator.fg),
        ));
    }
    Line::from(vec![
        Span::styled("──", Style::default().fg(theme.diff.separator.fg)),
        Span::styled(
            "─".repeat(width - 4),
            Style::default().fg(theme.input.hint.fg),
        ),
        Span::styled("──", Style::default().fg(theme.diff.separator.fg)),
    ])
}

pub fn render(frame: &mut Frame, area: Rect, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    render_at(frame, area, area.y, theme);
}

pub fn render_at(frame: &mut Frame, area: Rect, y: u16, theme: &Theme) {
    if area.width == 0 || y < area.y || y >= area.bottom() {
        return;
    }
    for offset in 0..area.width {
        if let Some(cell) = frame
            .buffer_mut()
            .cell_mut(Position::new(area.x.saturating_add(offset), y))
        {
            cell.set_symbol("─")
                .set_fg(rule_color(offset, area.width, theme));
        }
    }
}

fn rule_color(offset: u16, width: u16, theme: &Theme) -> Color {
    if offset < 2 || offset >= width.saturating_sub(2) {
        theme.diff.separator.fg
    } else {
        theme.input.hint.fg
    }
}
