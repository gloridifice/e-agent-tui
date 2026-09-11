//! Screen composition, responsive pane rectangles, global overlays, and
//! central cursor placement.

use ratatui::{
    layout::{Alignment, Position, Rect},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::{
    app::TuiApp,
    config::PaneWidthPercent,
    input::InputState,
    input_page::InputPageSession,
    interaction::{
        PaneResizeState, ScrollState, MIN_PREVIEW_COLUMNS,
        MIN_PREVIEW_PANE_WIDTH as MIN_PREVIEW_RECT_COLUMNS, PREVIEW_RIGHT_MARGIN_COLUMNS,
        PREVIEW_SEPARATOR_COLUMNS, PREVIEW_SEPARATOR_GAP_COLUMNS,
    },
    login::LoginState,
    settings::SettingsState,
    theme::Theme,
};

use super::layout::{main_page_rect, MAIN_PAGE_MARGIN};
use super::{overlay, pane};

/// Minimum usable Preview content width at the split threshold.
pub const MIN_PREVIEW_CONTENT_COLUMNS: u16 = MIN_PREVIEW_COLUMNS;
/// Preview remains beside the message pane only when its raw rectangle has
/// enough room for the separator, one post-separator gap, usable content, and
/// one right margin. Narrow layouts keep the existing full-screen toggle.
pub const MIN_PREVIEW_PANE_WIDTH: u16 = MIN_PREVIEW_RECT_COLUMNS;
/// Split Preview starts with the separator column plus one blank column before
/// its usable content. Full-screen Preview uses one ordinary edge column.
pub const PREVIEW_SPLIT_LEFT_PADDING: u16 =
    PREVIEW_SEPARATOR_COLUMNS + PREVIEW_SEPARATOR_GAP_COLUMNS;
pub const PREVIEW_SPLIT_RIGHT_PADDING: u16 = PREVIEW_RIGHT_MARGIN_COLUMNS;
pub const PREVIEW_FULLSCREEN_LEFT_PADDING: u16 = 1;
pub const PREVIEW_FULLSCREEN_RIGHT_PADDING: u16 = 1;
/// Ordinary Main content keeps one blank column at each pane edge.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenLayout {
    MainOnly(Rect),
    Split { main: Rect, preview: Rect },
    PreviewOnly(Rect),
}

pub fn layout(
    area: Rect,
    message_pane_percent: PaneWidthPercent,
    preview_fullscreen: bool,
) -> ScreenLayout {
    let main_width = message_pane_percent.columns(area.width);
    let preview_width = area.width.saturating_sub(main_width);
    if preview_width >= MIN_PREVIEW_PANE_WIDTH {
        ScreenLayout::Split {
            main: Rect::new(area.x, area.y, main_width, area.height),
            preview: Rect::new(area.x + main_width, area.y, preview_width, area.height),
        }
    } else if preview_fullscreen {
        ScreenLayout::PreviewOnly(area)
    } else {
        ScreenLayout::MainOnly(area)
    }
}

/// Return the terminal column occupied by the separator hit target. In a
/// collapsed main-only layout the target sits inside the right margin; in
/// Preview-only mode there is no resizable boundary.
pub fn separator_column(
    area: Rect,
    message_pane_percent: PaneWidthPercent,
    preview_fullscreen: bool,
) -> Option<u16> {
    match layout(area, message_pane_percent, preview_fullscreen) {
        ScreenLayout::Split { main, .. } => Some(main.right()),
        ScreenLayout::MainOnly(_) => area.width.checked_sub(2).map(|x| area.x + x),
        ScreenLayout::PreviewOnly(_) => None,
    }
}

pub fn separator_hit(
    area: Rect,
    message_pane_percent: PaneWidthPercent,
    preview_fullscreen: bool,
    column: u16,
) -> bool {
    separator_column(area, message_pane_percent, preview_fullscreen)
        .is_some_and(|target| target.abs_diff(column) <= 1)
}

fn pane_box(area: Rect, left_padding: u16, right_padding: u16) -> Option<Rect> {
    let width = area
        .width
        .saturating_sub(left_padding.saturating_add(right_padding));
    let height = area.height.saturating_sub(2);
    (width > 0 && height > 0).then(|| {
        Rect::new(
            area.x.saturating_add(left_padding),
            area.y + 1,
            width,
            height,
        )
    })
}

fn render_placeholder_box(
    frame: &mut Frame,
    area: Rect,
    left_padding: u16,
    right_padding: u16,
    label: &str,
    theme: &Theme,
) {
    let Some(area) = pane_box(area, left_padding, right_padding) else {
        return;
    };
    let placeholder = theme.separator.placeholder;
    let placeholder_style = placeholder.style();
    frame.render_widget(Block::default().style(placeholder_style), area);
    frame.render_widget(
        Paragraph::new(label.to_string())
            .alignment(Alignment::Center)
            .style(placeholder_style),
        area,
    );
}

fn paint_separator(frame: &mut Frame, area: Rect, column: u16, theme: &Theme, dragging: bool) {
    if column < area.x || column >= area.right() || area.height == 0 {
        return;
    }
    let bar = theme.separator.bar;
    let line = theme.separator.line;
    let bar_bg = bar.bg.unwrap_or(ratatui::style::Color::Reset);
    let line_bg = line.bg.unwrap_or(ratatui::style::Color::Reset);
    let center = area.y + area.height / 2;
    let buffer = frame.buffer_mut();
    if dragging {
        for row in area.y..area.bottom() {
            buffer[(column, row)]
                .set_symbol("│")
                .set_fg(line.fg)
                .set_bg(line_bg);
        }
    }
    let grip_height = if dragging { 5 } else { 3 };
    let start = center.saturating_sub(grip_height / 2);
    let end = (start + grip_height).min(area.bottom());
    for row in start..end {
        buffer[(column, row)]
            .set_symbol(if dragging { "┃" } else { "│" })
            .set_fg(bar.fg)
            .set_bg(bar_bg);
    }
}

fn render_resize_placeholder(
    frame: &mut Frame,
    area: Rect,
    resize: PaneResizeState,
    theme: &Theme,
    language: crate::Language,
) -> Option<Position> {
    let drag = resize.drag()?;
    frame.render_widget(Block::default().style(theme.surface.base.style()), area);

    let display_percent = if drag.pending_collapsed {
        PaneWidthPercent::from_basis_points(PaneWidthPercent::MAX_BASIS_POINTS)
            .expect("percentage maximum is valid")
    } else {
        drag.pending_percent
    };
    let main = if drag.pending_collapsed {
        area
    } else {
        match layout(area, drag.pending_percent, false) {
            ScreenLayout::Split { main, .. } | ScreenLayout::MainOnly(main) => main,
            ScreenLayout::PreviewOnly(_) => area,
        }
    };
    let main_right_padding = if drag.pending_collapsed {
        PREVIEW_SEPARATOR_COLUMNS + PREVIEW_SEPARATOR_GAP_COLUMNS + PREVIEW_RIGHT_MARGIN_COLUMNS
    } else {
        MAIN_PAGE_MARGIN
    };
    render_placeholder_box(
        frame,
        main,
        MAIN_PAGE_MARGIN,
        main_right_padding,
        &format!(
            "{}\n{}",
            crate::i18n::tr(language, "screen.message_pane"),
            display_percent.display()
        ),
        theme,
    );
    if !drag.pending_collapsed {
        if let ScreenLayout::Split { preview, .. } = layout(area, drag.pending_percent, false) {
            let preview_percent = 100.0 - drag.pending_percent.as_percent();
            render_placeholder_box(
                frame,
                preview,
                PREVIEW_SPLIT_LEFT_PADDING,
                PREVIEW_SPLIT_RIGHT_PADDING,
                &format!(
                    "{}\n{preview_percent:.2}%",
                    crate::i18n::tr(language, "screen.preview_pane")
                ),
                theme,
            );
        }
    }
    if let Some(column) = separator_column(area, display_percent, false) {
        paint_separator(frame, area, column, theme, true);
    }
    None
}

/// Screen-owned overlay and blocking-page handles. The Screen translates this
/// into a downward-only pane view rather than letting Panes import the Screen.
pub struct RenderOverlays<'a> {
    pub help_visible: bool,
    pub help_scroll: Option<&'a mut crate::interaction::HelpScrollState>,
    pub toast: Option<&'a str>,
    pub input_page: Option<&'a mut InputPageSession>,
    pub settings: Option<&'a mut SettingsState>,
    pub login: Option<&'a mut LoginState>,
    /// Live approval card and pending-prompt queue. These live in the
    /// InteractionModel, which the main loop holds outside AppState while
    /// rendering, so they are passed in instead of read off `state`.
    pub approval: Option<&'a crate::interaction::ApprovalCard>,
    pub queue: &'a [crate::interaction::PendingPrompt],
    /// Transient separator state copied from InteractionModel for rendering.
    /// It is intentionally absent from TuiApp semantic/cache state.
    pub pane_resize: PaneResizeState,
}

pub(super) fn render_with_cursor(
    frame: &mut Frame,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: RenderOverlays<'_>,
) -> Option<Position> {
    let pane_resize = overlays.pane_resize;
    if pane_resize.is_active() {
        return render_resize_placeholder(
            frame,
            frame.area(),
            pane_resize,
            theme,
            state.config.language,
        );
    }
    let RenderOverlays {
        help_visible,
        help_scroll,
        toast: _,
        input_page,
        settings,
        login,
        approval,
        queue,
        pane_resize: _,
    } = overlays;
    let pane_overlays = pane::main::MainPaneOverlays {
        input_page,
        settings,
        login,
        approval,
        queue,
    };
    let preview_fullscreen = state.preview.fullscreen && state.history_page.is_none();
    let screen_layout = layout(
        frame.area(),
        state.config.message_pane_percent,
        preview_fullscreen,
    );
    let cursor = match screen_layout {
        ScreenLayout::MainOnly(main) | ScreenLayout::Split { main, .. } => {
            if let Some(history_page) = &mut state.history_page {
                pane::history::render(frame, main, history_page, &state.config, theme);
                None
            } else {
                let collapsed = matches!(screen_layout, ScreenLayout::MainOnly(_));
                let cursor = pane::main::render_with_cursor(
                    frame,
                    main,
                    state,
                    input,
                    scroll,
                    theme,
                    pane_overlays,
                    collapsed,
                );
                state.rebuild_reading_model();
                render_reading_rail(frame, main, state, scroll, theme, collapsed);
                render_reading_item(frame, main, state, scroll, theme, collapsed);
                cursor
            }
        }
        ScreenLayout::PreviewOnly(_) => None,
    };
    match screen_layout {
        ScreenLayout::Split { preview, .. } => pane::preview::render(
            frame,
            preview,
            &mut state.preview,
            &state.config,
            theme,
            PREVIEW_SPLIT_LEFT_PADDING,
            PREVIEW_SPLIT_RIGHT_PADDING,
        ),
        ScreenLayout::PreviewOnly(preview) => pane::preview::render(
            frame,
            preview,
            &mut state.preview,
            &state.config,
            theme,
            PREVIEW_FULLSCREEN_LEFT_PADDING,
            PREVIEW_FULLSCREEN_RIGHT_PADDING,
        ),
        ScreenLayout::MainOnly(_) => {}
    }
    if !matches!(screen_layout, ScreenLayout::PreviewOnly(_)) {
        if let Some(column) = separator_column(
            frame.area(),
            state.config.message_pane_percent,
            preview_fullscreen,
        ) {
            paint_separator(frame, frame.area(), column, theme, false);
        }
    }
    if help_visible {
        overlay::render_help_modal(frame, &state.config, theme, help_scroll);
        None
    } else {
        cursor
    }
}

fn render_reading_rail(
    frame: &mut Frame,
    main: Rect,
    state: &TuiApp,
    scroll: &ScrollState,
    theme: &Theme,
    reserve_collapsed_separator: bool,
) {
    let Some(rows) = state
        .reading
        .as_ref()
        .and_then(|reading| state.reading_layout.block(&reading.block_cursor))
        .map(|block| block.rows.clone())
    else {
        return;
    };
    let rail_x = main_page_rect(main, state, reserve_collapsed_separator)
        .x
        .saturating_sub(1);
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
    reserve_collapsed_separator: bool,
) {
    let Some((item_id, geometry)) = state.reading.as_ref().and_then(|reading| {
        let item = reading.item_cursor.as_ref()?;
        let block = state.reading_layout.block(&reading.block_cursor)?;
        Some((item, block))
    }) else {
        return;
    };
    let page = main_page_rect(main, state, reserve_collapsed_separator);
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
    fn wide_and_narrow_rectangles_obey_percentage_and_preview_minimums() {
        let wide = Rect::new(0, 0, 120, 40);
        assert_eq!(
            layout(wide, PaneWidthPercent::default(), false),
            ScreenLayout::Split {
                main: Rect::new(0, 0, 72, 40),
                preview: Rect::new(72, 0, 48, 40),
            }
        );
        let narrow = Rect::new(0, 0, 20, 30);
        assert_eq!(
            layout(narrow, PaneWidthPercent::default(), false),
            ScreenLayout::MainOnly(narrow)
        );
        assert_eq!(
            layout(narrow, PaneWidthPercent::default(), true),
            ScreenLayout::PreviewOnly(narrow)
        );
    }

    #[test]
    fn separator_hit_uses_the_boundary_or_collapsed_right_margin() {
        let area = Rect::new(0, 0, 120, 40);
        assert_eq!(
            separator_column(area, PaneWidthPercent::default(), false),
            Some(72)
        );
        assert!(separator_hit(area, PaneWidthPercent::default(), false, 71));
        assert!(separator_hit(area, PaneWidthPercent::default(), false, 73));
        assert!(!separator_hit(area, PaneWidthPercent::default(), false, 68));

        let collapsed = PaneWidthPercent::from_basis_points(10_000).unwrap();
        assert_eq!(separator_column(area, collapsed, false), Some(118));
        assert!(separator_hit(area, collapsed, false, 117));
        assert_eq!(separator_column(area, collapsed, true), None);
    }

    #[test]
    fn exactly_sixteen_preview_content_columns_still_split() {
        let percent = PaneWidthPercent::default();
        let split = layout(Rect::new(0, 0, 47, 20), percent, false);
        assert!(matches!(
            split,
            ScreenLayout::Split {
                main: Rect { width: 28, .. },
                preview: Rect { width: 19, .. },
            }
        ));
        assert_eq!(
            MIN_PREVIEW_PANE_WIDTH
                .saturating_sub(PREVIEW_SPLIT_LEFT_PADDING)
                .saturating_sub(PREVIEW_SPLIT_RIGHT_PADDING),
            MIN_PREVIEW_CONTENT_COLUMNS
        );
        assert!(matches!(
            layout(Rect::new(0, 0, 46, 20), percent, false),
            ScreenLayout::MainOnly(_)
        ));
    }

    #[test]
    fn main_page_insets_leave_one_column_and_reserve_collapsed_grip() {
        let state = TuiApp::default();
        assert_eq!(
            main_page_rect(Rect::new(0, 0, 72, 20), &state, false),
            Rect::new(1, 0, 70, 20)
        );
        assert_eq!(
            main_page_rect(Rect::new(0, 0, 120, 20), &state, true),
            Rect::new(1, 0, 116, 20)
        );
    }
}
