//! Input Page renderers.

mod login;
mod model;
mod question;
mod resume;
mod settings;
mod theme;

pub(super) use login::render_login;
use login::render_login_scrolled;
use model::render_model_page;
use question::render_question_page;
use resume::render_resume_page;
pub(super) use settings::render_settings;
use theme::render_theme_page;

use unicode_segmentation::UnicodeSegmentation;

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
) -> Option<Position> {
    match &mut session.page {
        InputPage::Settings(settings) => {
            render_settings(frame, area, settings, config, theme);
            None
        }
        InputPage::Login(login) => {
            render_login_scrolled(frame, area, login, &mut session.viewport, theme);
            None
        }
        InputPage::Model(model) => {
            render_model_page(
                frame,
                area,
                model,
                &session.focus,
                &mut session.viewport,
                theme,
            );
            None
        }
        InputPage::Theme(page) => {
            render_theme_page(
                frame,
                area,
                page,
                &session.focus,
                &mut session.viewport,
                theme,
            );
            None
        }
        InputPage::Resume(page) => {
            render_resume_page(frame, area, page, &mut session.viewport, theme);
            None
        }
        InputPage::Question(batch) => render_question_page(
            frame,
            area,
            batch,
            &session.focus,
            &mut session.viewport,
            theme,
        ),
    }
}

/// Wrap `text` into rows of at most `width` display columns. Grapheme
/// clusters (combining marks, emoji ZWJ sequences, flags) are never split
/// across rows; a single cluster wider than `width` still occupies its own
/// row.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0usize;
    for grapheme in text.graphemes(true) {
        let w = UnicodeWidthStr::width(grapheme);
        if used + w > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push_str(grapheme);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_text_splits_at_exact_display_width() {
        assert_eq!(wrap_text("abcdef", 3), vec!["abc", "def"]);
        assert_eq!(wrap_text("abc", 3), vec!["abc"]);
        assert_eq!(wrap_text("", 3), Vec::<String>::new());
        // CJK double-width glyphs count as two columns.
        assert_eq!(wrap_text("你好世界", 4), vec!["你好", "世界"]);
    }

    #[test]
    fn wrap_text_keeps_zwj_and_combining_graphemes_intact() {
        // Family emoji is one grapheme cluster of 2 display columns; a narrow
        // box must keep it whole instead of splitting mid-cluster at a char
        // boundary.
        let family = "👨‍👩‍👧";
        assert_eq!(wrap_text(family, 2), vec![family.to_string()]);
        assert_eq!(
            wrap_text(&format!("{family}{family}"), 3),
            vec![family.to_string(); 2],
            "two clusters split cleanly at the cluster boundary"
        );
        // A combining mark stays glued to its base char.
        assert_eq!(
            wrap_text("e\u{301}x", 1),
            vec!["e\u{301}".to_string(), "x".to_string()]
        );
    }
}
