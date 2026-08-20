//! Screen composition, responsive pane rectangles, global overlays, and
//! central cursor placement.

use ratatui::{
    layout::{Position, Rect},
    Frame,
};

use crate::{
    app::TuiApp, input::InputState, input_page::InputPageSession, interaction::ScrollState,
    login::LoginState, settings::SettingsState, theme::Theme,
};

use super::pane;

/// Characterization tests keep transcript, composer, and status usable at
/// this effective main-pane width.
pub const MIN_MAIN_PANE_WIDTH: u16 = 40;
pub const MIN_PREVIEW_PANE_WIDTH: u16 = 32;
/// Conflict audit: Ctrl+P is unclaimed by composer, pages, approvals, copy
/// mode, and global help, so it owns narrow full-screen Preview toggling.
pub const PREVIEW_TOGGLE_KEY: char = 'p';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenLayout {
    MainOnly(Rect),
    Split { main: Rect, preview: Rect },
    PreviewOnly(Rect),
}

pub fn layout(area: Rect, configured_main_width: usize, preview_fullscreen: bool) -> ScreenLayout {
    let configured = if configured_main_width == 0 {
        area.width
    } else {
        configured_main_width.min(u16::MAX as usize) as u16
    };
    let main_width = ((u32::from(area.width) * 60) / 100) as u16;
    let main_width = main_width.min(configured);
    if main_width >= MIN_MAIN_PANE_WIDTH
        && area.width.saturating_sub(main_width) >= MIN_PREVIEW_PANE_WIDTH
    {
        ScreenLayout::Split {
            main: Rect::new(area.x, area.y, main_width, area.height),
            preview: Rect::new(
                area.x + main_width,
                area.y,
                area.width - main_width,
                area.height,
            ),
        }
    } else if preview_fullscreen {
        ScreenLayout::PreviewOnly(area)
    } else {
        ScreenLayout::MainOnly(area)
    }
}

/// Screen-owned overlay and blocking-page handles. The Screen translates this
/// into a downward-only pane view rather than letting Panes import the Screen.
pub struct RenderOverlays<'a> {
    pub help_visible: bool,
    pub toast: Option<&'a str>,
    pub input_page: Option<&'a mut InputPageSession>,
    pub settings: Option<&'a mut SettingsState>,
    pub login: Option<&'a mut LoginState>,
}

pub(super) fn render_with_cursor(
    frame: &mut Frame,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: RenderOverlays<'_>,
) -> Option<Position> {
    let RenderOverlays {
        help_visible,
        toast,
        input_page,
        settings,
        login,
    } = overlays;
    let pane_overlays = pane::main::MainPaneOverlays {
        help_visible,
        toast,
        input_page,
        settings,
        login,
    };
    match layout(
        frame.area(),
        state.config.main_pane_width,
        state.preview.fullscreen,
    ) {
        ScreenLayout::MainOnly(main) => {
            let cursor = pane::main::render_with_cursor(
                frame,
                main,
                state,
                input,
                scroll,
                theme,
                pane_overlays,
            );
            state.rebuild_reading_model();
            render_reading_rail(frame, main, state, scroll, theme);
            render_reading_item(frame, main, state, scroll, theme);
            cursor
        }
        ScreenLayout::Split { main, preview } => {
            let cursor = pane::main::render_with_cursor(
                frame,
                main,
                state,
                input,
                scroll,
                theme,
                pane_overlays,
            );
            state.rebuild_reading_model();
            render_reading_rail(frame, main, state, scroll, theme);
            render_reading_item(frame, main, state, scroll, theme);
            pane::preview::render(frame, preview, &mut state.preview, theme);
            cursor
        }
        ScreenLayout::PreviewOnly(preview) => {
            pane::preview::render(frame, preview, &mut state.preview, theme);
            None
        }
    }
}

/// Content (page) width for a main pane: `max_width` capped by the pane width
/// minus the fixed side margins (alignment shifts only x, never the width).
fn content_page_width(main: Rect, max_width: u16) -> u16 {
    const MARGIN: u16 = 4;
    let available = main.width.saturating_sub(MARGIN * 2);
    if max_width > 0 {
        max_width.min(available)
    } else {
        available
    }
}

/// Content page rectangle for a main pane, owning the margin/cap/align policy.
/// Shared by rendering and the runtime scroll path (`ui::input_bar_width`) so
/// both resolve the same content width.
pub(super) fn main_page_rect(main: Rect, state: &TuiApp) -> Rect {
    const MARGIN: u16 = 4;
    let max_width = state.config.page_max_width as u16;
    let width = content_page_width(main, max_width);
    let x = match state.config.page_align.as_str() {
        "left" => main.x + MARGIN,
        "right" => main.x + main.width.saturating_sub(width.saturating_add(MARGIN)),
        _ => main.x + main.width.saturating_sub(width) / 2,
    };
    Rect::new(x, main.y, width, main.height)
}

fn render_reading_rail(
    frame: &mut Frame,
    main: Rect,
    state: &TuiApp,
    scroll: &ScrollState,
    theme: &Theme,
) {
    let Some(rows) = state
        .reading
        .as_ref()
        .and_then(|reading| state.reading_layout.block(&reading.block_cursor))
        .map(|block| block.rows.clone())
    else {
        return;
    };
    let rail_x = main_page_rect(main, state).x.saturating_sub(1);
    if rail_x >= main.right() {
        return;
    }
    let buffer = frame.buffer_mut();
    for global_row in rows {
        let Some(local_row) = global_row.checked_sub(scroll.offset) else {
            continue;
        };
        if local_row >= usize::from(main.height) {
            continue;
        }
        let y = main.y + local_row as u16;
        buffer[(rail_x, y)]
            .set_symbol("│")
            .set_fg(theme.overlay.border.fg)
            .set_bg(theme.bg);
    }
}

fn render_reading_item(
    frame: &mut Frame,
    main: Rect,
    state: &TuiApp,
    scroll: &ScrollState,
    theme: &Theme,
) {
    let Some((item_id, geometry)) = state.reading.as_ref().and_then(|reading| {
        let item = reading.item_cursor.as_ref()?;
        let block = state.reading_layout.block(&reading.block_cursor)?;
        Some((item, block))
    }) else {
        return;
    };
    let page = main_page_rect(main, state);
    let buffer = frame.buffer_mut();
    for fragment in geometry
        .items
        .iter()
        .filter(|fragment| &fragment.item_id == item_id)
    {
        let Some(local_row) = fragment.row.checked_sub(scroll.offset) else {
            continue;
        };
        if local_row >= usize::from(page.height) {
            continue;
        }
        let y = page.y + local_row as u16;
        for local_x in fragment.x.clone() {
            if local_x >= usize::from(page.width) {
                break;
            }
            let cell = &mut buffer[(page.x + local_x as u16, y)];
            if matches!(cell.bg, ratatui::style::Color::Reset) || cell.bg == theme.bg {
                cell.set_bg(theme.selection);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_and_narrow_rectangles_obey_minimums() {
        let wide = Rect::new(0, 0, 120, 40);
        assert_eq!(
            layout(wide, 120, false),
            ScreenLayout::Split {
                main: Rect::new(0, 0, 72, 40),
                preview: Rect::new(72, 0, 48, 40),
            }
        );
        let narrow = Rect::new(0, 0, 70, 30);
        assert_eq!(layout(narrow, 120, false), ScreenLayout::MainOnly(narrow));
        assert_eq!(layout(narrow, 120, true), ScreenLayout::PreviewOnly(narrow));
    }
}
