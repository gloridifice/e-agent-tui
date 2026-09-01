use ratatui::{
    layout::{Position, Rect},
    Frame,
};

use crate::{
    app::TuiApp, input::InputState, input_page::InputPageSession, interaction::ScrollState,
    login::LoginState, mouse_selection::SelectionFrame, settings::SettingsState, theme::Theme,
};

pub(crate) struct MainPaneOverlays<'a> {
    pub help_visible: bool,
    pub input_page: Option<&'a mut InputPageSession>,
    pub settings: Option<&'a mut SettingsState>,
    pub login: Option<&'a mut LoginState>,
    pub approval: Option<&'a crate::interaction::ApprovalCard>,
    pub queue: &'a [crate::PromptInput],
}

#[allow(clippy::too_many_arguments)] // Thin downward-only forwarding boundary.
pub(crate) fn render_with_cursor(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: MainPaneOverlays<'_>,
    selection_frame: &mut SelectionFrame,
    reserve_collapsed_separator: bool,
) -> Option<Position> {
    super::super::render_main_pane_with_cursor(
        frame,
        area,
        state,
        input,
        scroll,
        theme,
        overlays,
        selection_frame,
        reserve_collapsed_separator,
    )
}
