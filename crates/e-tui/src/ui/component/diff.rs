use ratatui::style::Style;

use crate::theme::Theme;

pub fn added(theme: &Theme) -> Style {
    theme.working_status.success.style()
}

pub fn removed(theme: &Theme) -> Style {
    theme.working_status.failure.style()
}
