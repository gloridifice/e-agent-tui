use std::time::Duration;

use ratatui::{style::Color, text::Span};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    app::TuiApp,
    display::{ActivityRow, ActivityState},
    theme::Theme,
};

pub const SETTLED_INDICATOR: &str = "•";

const THINKING_SWEEP_SECONDS: f64 = 1.6;
const THINKING_PAUSE_SECONDS: f64 = 0.2;
const THINKING_HIGHLIGHT_RADIUS: f64 = 0.5 / 0.13;

pub fn thinking_label_spans(label: &str, theme: &Theme, elapsed: Duration) -> Vec<Span<'static>> {
    let cycle_time = elapsed.as_secs_f64() % (THINKING_SWEEP_SECONDS + THINKING_PAUSE_SECONDS);
    let sweeping = cycle_time < THINKING_SWEEP_SECONDS;
    let progress = (cycle_time / THINKING_SWEEP_SECONDS).min(1.0);
    let graphemes = label.graphemes(true);
    let travel =
        graphemes.clone().count().saturating_sub(1) as f64 + 2.0 * THINKING_HIGHLIGHT_RADIUS;
    let center = -THINKING_HIGHLIGHT_RADIUS + progress * travel;
    let style = theme.activity.label.style();
    graphemes
        .enumerate()
        .map(|(index, grapheme)| {
            let distance = (index as f64 - center) / THINKING_HIGHLIGHT_RADIUS;
            // A single crest enters and leaves the label completely before the pause.
            let level = if sweeping && distance.abs() < 1.0 {
                ((distance * std::f64::consts::PI).cos() + 1.0) / 2.0
            } else {
                0.0
            };
            let color =
                crate::color::lerp_rgb(theme.activity.label.fg, theme.activity.detail.fg, level);
            Span::styled(grapheme.to_owned(), style.fg(color))
        })
        .collect()
}

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
