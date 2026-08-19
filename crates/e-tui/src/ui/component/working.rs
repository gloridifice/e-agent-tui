use ratatui::style::Color;

use crate::{
    app::{breathing_color, settle_color, TuiApp},
    display::{ActivityRow, ActivityState},
    theme::Theme,
};

pub fn activity_color(
    theme: &Theme,
    state: &TuiApp,
    row: &ActivityRow,
    color_override: Option<Color>,
) -> Color {
    let target = match row.state {
        ActivityState::Waiting => theme.working_status.waiting.fg,
        ActivityState::Running => breathing_color(theme, state.breath_phase()),
        ActivityState::Success => theme.working_status.success.fg,
        ActivityState::Failure => theme.working_status.failure.fg,
        ActivityState::Cancelled => theme.working_status.cancelled.fg,
    };
    color_override.unwrap_or_else(|| {
        state
            .render
            .activity_transitions
            .get(&row.id)
            .map_or(target, |transition| {
                settle_color(transition.from, target, transition.done_since.elapsed())
            })
    })
}
