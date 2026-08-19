use ratatui::style::Style;

use crate::theme::Theme;

pub fn dim(theme: &Theme) -> Style {
    theme.input.status_hint.style()
}

pub fn accent(theme: &Theme) -> Style {
    theme.input.status_accent.style()
}
