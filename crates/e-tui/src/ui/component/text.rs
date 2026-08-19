use ratatui::style::Color;

use crate::{display::DisplayTone, theme::Theme};

pub fn tone_color(theme: &Theme, tone: DisplayTone) -> Color {
    match tone {
        DisplayTone::Normal => theme.surface.primary_text.fg,
        DisplayTone::Dim => theme.surface.muted_text.fg,
        DisplayTone::Info => theme.log.info.fg,
        DisplayTone::Warning => theme.log.warning.fg,
        DisplayTone::Error => theme.log.error.fg,
    }
}
