use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::{i18n::Language, input::InputState, theme::Theme};
use unicode_width::UnicodeWidthStr;

fn status_label<'a>(
    input: &InputState,
    shell_name: &'a str,
    macros: &crate::prompt_macros::PromptMacros,
) -> &'a str {
    if input.matched_macro(macros).is_some() {
        "macro"
    } else if input.is_shell_command() {
        shell_name
    } else {
        ""
    }
}

pub(crate) fn tips_width(
    width: u16,
    input: &InputState,
    shell_name: &str,
    macros: &crate::prompt_macros::PromptMacros,
) -> usize {
    let label = status_label(input, shell_name, macros);
    let reserved = if label.is_empty() {
        0
    } else {
        (label.width() + 2).min(usize::from(width.saturating_sub(4))) + 1
    };
    usize::from(width.saturating_sub(6)).saturating_sub(reserved)
}

pub(crate) fn render_link_copy_hint(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    language: Language,
) {
    frame.render_widget(
        Paragraph::new(crate::i18n::tr(language, "input.link_copy_hint"))
            .style(Style::default().fg(theme.ok)),
        area,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    input: &InputState,
    theme: &Theme,
    horizontal_padding: u16,
    model_hint: Option<&str>,
    catalogs: &crate::CatalogModel,
    shell_name: &str,
    macros: &crate::prompt_macros::PromptMacros,
    tip: Option<&crate::tips::TipLine>,
    focused: bool,
) -> Option<Position> {
    let cursor = super::super::input::render_focused_input(
        frame,
        area,
        input,
        theme,
        horizontal_padding,
        model_hint,
        catalogs,
        macros,
        focused,
    );
    let label = status_label(input, shell_name, macros);
    if !label.is_empty() && area.width > 4 && area.height > 1 {
        frame.buffer_mut().set_line(
            area.x + 2,
            area.bottom() - 1,
            &Line::styled(format!(" {label} "), theme.input.status_hint.style()),
            area.width - 4,
        );
    }
    if area.height > 1 {
        let budget = tips_width(area.width, input, shell_name, macros);
        if let Some(tip) =
            tip.filter(|tip| tip.budget == budget && budget >= crate::tips::MIN_TIP_COLUMNS)
        {
            let style = Style::default().fg(theme.activity.detail.fg);
            let mut spans = vec![Span::raw(" ")];
            spans.extend(tip.segments.iter().map(|(text, keyword)| {
                Span::styled(
                    text.as_str(),
                    if *keyword {
                        style.add_modifier(Modifier::BOLD | Modifier::ITALIC)
                    } else {
                        style
                    },
                )
            }));
            spans.push(Span::raw(" "));
            let line = Line::from(spans);
            frame.buffer_mut().set_line(
                area.right() - 4 - tip.width as u16,
                area.bottom() - 1,
                &line,
                tip.width as u16 + 2,
            );
        }
    }
    cursor
}
