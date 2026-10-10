use ratatui::{
    layout::{Position, Rect},
    style::Style,
    text::Line,
    widgets::Paragraph,
    Frame,
};

use crate::{i18n::Language, input::InputState, theme::Theme};

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
) -> Option<Position> {
    let cursor = super::super::input::render_input_with_catalog(
        frame,
        area,
        input,
        theme,
        horizontal_padding,
        model_hint,
        catalogs,
    );
    if input.is_shell_command() && !shell_name.is_empty() && area.width > 4 && area.height > 1 {
        frame.buffer_mut().set_line(
            area.x + 2,
            area.bottom() - 1,
            &Line::styled(format!(" {shell_name} "), theme.input.status_hint.style()),
            area.width - 4,
        );
    }
    cursor
}
