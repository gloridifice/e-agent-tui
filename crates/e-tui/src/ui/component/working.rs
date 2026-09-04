use ratatui::style::Color;

use crate::{
    app::TuiApp,
    display::{ActivityRow, ActivityState},
    theme::Theme,
};

pub const SETTLED_INDICATOR: &str = "•";

pub fn activity_indicator(state: &TuiApp, row: &ActivityRow) -> &'static str {
    if row.state == ActivityState::Running {
        state.activity_spinner_frame()
    } else {
        SETTLED_INDICATOR
    }
}

pub fn activity_color(theme: &Theme, row: &ActivityRow, color_override: Option<Color>) -> Color {
    color_override.unwrap_or(match row.state {
        ActivityState::Waiting => theme.working_status.waiting.fg,
        ActivityState::Running => theme.working_status.running.fg,
        ActivityState::Success => theme.activity.label.fg,
        ActivityState::Failure => theme.working_status.failure.fg,
        ActivityState::Cancelled => theme.working_status.cancelled.fg,
    })
}
