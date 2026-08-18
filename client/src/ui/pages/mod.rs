//! Input Page renderers.

mod login;
mod model;
mod resume;
mod settings;
mod theme;

pub(super) use login::render_login;
use login::render_login_scrolled;
use model::render_model_page;
use resume::render_resume_page;
pub(super) use settings::render_settings;
use theme::render_theme_page;

use super::*;

fn input_page_shell(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    theme: &Theme,
) -> InputPageRegions {
    let block = Block::default()
        .style(Style::default().bg(theme.bg_soft))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);
    InputPageRegions {
        header: rows[0],
        body: rows[2],
        footer: rows[3],
    }
}

pub(super) fn render_input_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    session: &mut InputPageSession,
    config: &crate::config::Config,
    theme: &Theme,
) {
    match &mut session.page {
        InputPage::Settings(settings) => render_settings(frame, area, settings, config, theme),
        InputPage::Login(login) => {
            render_login_scrolled(frame, area, login, &mut session.viewport, theme)
        }
        InputPage::Model(model) => render_model_page(
            frame,
            area,
            model,
            &session.focus,
            &mut session.viewport,
            theme,
        ),
        InputPage::Theme(page) => render_theme_page(
            frame,
            area,
            page,
            &session.focus,
            &mut session.viewport,
            theme,
        ),
        InputPage::Resume(page) => {
            render_resume_page(frame, area, page, &mut session.viewport, theme)
        }
    }
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + w > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push(ch);
        used += w;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Trim `text` to at most `width` display columns, appending `…` when it
/// overflows (the ellipsis itself counts against the budget).
pub(super) fn trim_to_width(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + w + 1 > width {
            out.push('…');
            break;
        }
        used += w;
        out.push(ch);
    }
    out
}
