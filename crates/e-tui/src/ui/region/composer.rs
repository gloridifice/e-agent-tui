use ratatui::{
    layout::{Position, Rect},
    style::Style,
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

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    input: &InputState,
    theme: &Theme,
    horizontal_padding: u16,
    model_hint: Option<&str>,
) -> Option<Position> {
    super::super::input::render_input(frame, area, input, theme, horizontal_padding, model_hint)
}
