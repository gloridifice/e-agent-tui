use super::*;
use crate::i18n::{tr, Language};

pub(super) fn render_toast(frame: &mut Frame, message: &str, theme: &Theme) {
    let area = frame.area();
    if area.width < 4 || area.height < 3 {
        return;
    }
    let content = format!("✓ {message}");
    let width = (UnicodeWidthStr::width(content.as_str()) as u16)
        .saturating_add(4)
        .min(area.width);
    let right = area.x.saturating_add(area.width);
    let x = right.saturating_sub(width.saturating_add(1)).max(area.x);
    let y = if area.height > 3 {
        area.y.saturating_add(1)
    } else {
        area.y
    };
    let popup = ratatui::layout::Rect::new(x, y, width, 3);
    frame.render_widget(ratatui::widgets::Clear, popup);
    frame.render_widget(
        Paragraph::new(Line::styled(content, theme.working_status.success.style()))
            .alignment(ratatui::layout::Alignment::Center)
            .block(
                Block::bordered()
                    .style(theme.overlay.background.style())
                    .border_style(theme.overlay.border.style()),
            ),
        popup,
    );
}

pub(super) fn help_overlay(language: Language, theme: &Theme) -> Vec<Line<'static>> {
    let keys = [
        "overlay.help.title",
        "overlay.help.row1",
        "overlay.help.row2",
        "overlay.help.row3",
        "overlay.help.row4",
        "overlay.help.row5",
        "overlay.help.row6",
        "overlay.help.row7",
        "overlay.help.row8",
        "overlay.help.row9",
        "overlay.help.row10",
        "overlay.help.row11",
        "overlay.help.row12",
    ];
    keys.into_iter()
        .map(|key| {
            Line::from(Span::styled(
                tr(language, key),
                Style::default().fg(theme.fg).bg(theme.bg_soft),
            ))
        })
        .collect()
}
