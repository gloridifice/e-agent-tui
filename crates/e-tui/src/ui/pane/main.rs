use ratatui::{
    layout::{Position, Rect},
    Frame,
};

use crate::{
    app::TuiApp, input::InputState, input_page::InputPageSession, interaction::ScrollState,
    login::LoginState, settings::SettingsState, theme::Theme,
};

pub(crate) struct MainPaneOverlays<'a> {
    pub help_visible: bool,
    pub toast: Option<&'a str>,
    pub input_page: Option<&'a mut InputPageSession>,
    pub settings: Option<&'a mut SettingsState>,
    pub login: Option<&'a mut LoginState>,
}

pub(crate) fn render_with_cursor(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: MainPaneOverlays<'_>,
) -> Option<Position> {
    super::super::render_main_pane_with_cursor(frame, area, state, input, scroll, theme, overlays)
}
