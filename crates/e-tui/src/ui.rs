//! Ratatui rendering: status bar, transcript, borderless input bar
//! (design §3.1, D23).

use ratatui::{
    layout::{Constraint, Layout, Position},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{breathing_color, TuiApp},
    cache::MessageLineRange,
    command_catalog::CommandSource,
    config::{Theme, ThinkingDisplayMode},
    display::{
        allocate_accessories, ActivityRow, CardRole, ContentCard, DisplayItem, DisplayTone,
        InputAccessory, InputAccessoryKind, ThinkingNode, TranscriptBlock, TranscriptFormat,
    },
    input::{InputState, Suggestion, SuggestionKind},
    input_page::{FocusId, InputPage, InputPageSession, ModelPage, ResumePage, ThemePage},
    login::LoginState,
    mouse_selection::{MouseSelection, SelectionFrame},
    projection::TranscriptNode,
    settings::SettingsState,
    transcript_layout::{truncate_activity_line, wrap_line, wrapped_rows, ProvenanceLayoutRow},
    SessionStatus as AgentStatus,
};
mod accessories;
/// Multiline input shows at most this many rows (D23).
pub mod component;
mod input;
mod overlay;
mod pages;
pub mod pane;
pub mod region;
pub mod screen;
pub(crate) mod selection;
mod status;
mod transcript;
use pages::{render_login, render_settings, trim_to_width, wrap_text};

use accessories::{
    render_approval, render_info_accessory, render_queue, render_suggest, render_todo,
};
use overlay::help_overlay;
use transcript::InputPageRegions;
pub use transcript::{provenance_layout_rows, scroll_lines, scroll_page};

const INPUT_MAX_ROWS: usize = 5;

pub use crate::interaction::ScrollState;

/// Guard against pathological transcripts (huge snapshots).
const MAX_RENDER_LINES_PER_MSG: usize = 800;

pub use screen::RenderOverlays;

fn input_accessories(
    state: &TuiApp,
    approval: Option<&crate::interaction::ApprovalCard>,
    queue: &[String],
) -> Vec<InputAccessory> {
    let mut accessories = Vec::new();
    if approval.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Approval,
            priority: 90,
            desired_rows: 3,
            minimum_rows: 3,
            blocking: true,
            insertion_order: 0,
        });
    }
    if state.goal.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Goal,
            priority: 40,
            desired_rows: 1,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 1,
        });
    }
    if state.plan_mode.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Plan,
            priority: 30,
            desired_rows: 1,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 2,
        });
    }
    if !state.todos.is_empty() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Todo,
            priority: 20,
            desired_rows: (state.todos.len() + 1).min(6) as u16,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 3,
        });
    }
    if !queue.is_empty() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Queue,
            priority: 10,
            desired_rows: queue.len().min(u16::MAX as usize) as u16,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 4,
        });
    }
    accessories
}

fn bottom_area_rows(
    area_height: u16,
    area_width: u16,
    input: &InputState,
    input_page_open: bool,
    padding: usize,
) -> u16 {
    if input_page_open {
        ((area_height as u32) * 2 / 3).min(area_height.saturating_sub(3) as u32) as u16
    } else {
        (input_rows(input, area_width as usize, padding) + 2) as u16
    }
}

/// Width of the input bar (the content page) for a terminal of `area_width`
/// columns, mirroring the split/page policy of rendering. The runtime scroll
/// path uses this so keyboard/mouse paging stays aligned with the rendered
/// input bar height, which grows with wrapped rows.
pub fn input_bar_width(area_width: u16, state: &TuiApp) -> u16 {
    let (main, reserve_collapsed_separator) = match screen::layout(
        ratatui::layout::Rect::new(0, 0, area_width, 0),
        state.config.message_pane_percent,
        state.preview.fullscreen,
    ) {
        screen::ScreenLayout::MainOnly(main) => (main, true),
        screen::ScreenLayout::Split { main, .. } => (main, false),
        screen::ScreenLayout::PreviewOnly(_) => return area_width,
    };
    screen::main_page_rect(main, state, reserve_collapsed_separator).width
}

/// Terminal dimensions shared by the render path and the runtime scroll/input
/// path. Named fields remove the transposition hazard of threading two
/// adjacent `u16`s (height,width vs width,height) across the render→runtime
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub width: u16,
    pub height: u16,
}

/// Visible transcript rows for the same bottom/accessory policy used by
/// `render`; keyboard and mouse scrolling must use this rather than the full
/// terminal height. The live approval card and pending-prompt queue are passed
/// in because the main loop may hold the InteractionModel out of AppState.
pub fn transcript_view_height(
    size: TerminalSize,
    state: &TuiApp,
    input: &InputState,
    input_page_open: bool,
    approval: Option<&crate::interaction::ApprovalCard>,
    queue: &[String],
) -> usize {
    let bottom_rows = bottom_area_rows(
        size.height,
        input_bar_width(size.width, state),
        input,
        input_page_open,
        state.config.user_input_padding,
    );
    let accessory_budget = size.height.saturating_sub(1 + bottom_rows + 3);
    let accessory_rows: u16 =
        allocate_accessories(&input_accessories(state, approval, queue), accessory_budget)
            .iter()
            .map(|item| item.rows)
            .sum();
    usize::from(
        size.height
            .saturating_sub(bottom_rows + accessory_rows + 3)
            .max(1),
    )
}

pub fn render(
    frame: &mut Frame,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: RenderOverlays<'_>,
) {
    let _ = render_with_cursor(frame, state, input, scroll, theme, overlays);
}

/// Render one frame and return the hidden terminal-cursor anchor used by IME.
/// The caller owns cursor visibility; UI code must never call
/// `Frame::set_cursor_position`, because ratatui would show the hardware cursor
/// while diff cells are being written and make it jump through animated rows.
pub fn render_with_cursor(
    frame: &mut Frame,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: RenderOverlays<'_>,
) -> Option<Position> {
    render_with_cursor_and_selection(
        frame,
        state,
        input,
        scroll,
        theme,
        overlays,
        &MouseSelection::default(),
        &SelectionFrame::default(),
    )
    .cursor
}

/// Ephemeral artifacts produced by one render attempt. The runner publishes
/// `selection_frame` only after terminal submission succeeds.
pub struct RenderOutput {
    pub cursor: Option<Position>,
    pub selection_frame: SelectionFrame,
}

#[allow(clippy::too_many_arguments)] // Explicit render inputs preserve the UI boundary.
pub fn render_with_cursor_and_selection(
    frame: &mut Frame,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: RenderOverlays<'_>,
    selection: &MouseSelection,
    committed_selection_frame: &SelectionFrame,
) -> RenderOutput {
    let resizing = overlays.pane_resize.is_active();
    let toast = (!resizing).then_some(overlays.toast).flatten();
    let area = frame.area();
    let mut selection_frame = SelectionFrame::for_viewport(area.width, area.height);
    let cursor = screen::render_with_cursor(
        frame,
        state,
        input,
        scroll,
        theme,
        overlays,
        &mut selection_frame,
    );
    // Never paint a range from stale coordinates onto a newly composed frame.
    // The runner publishes the new geometry only after terminal submission.
    if !resizing && selection_frame.same_geometry(committed_selection_frame) {
        selection::paint(committed_selection_frame, selection, frame.buffer_mut());
    }
    if let Some(toast) = toast {
        overlay::render_toast(frame, toast, theme);
    }
    RenderOutput {
        cursor,
        selection_frame,
    }
}

#[allow(clippy::too_many_arguments)] // Shared render inputs stay explicit at the UI boundary.
pub(crate) fn render_main_pane_with_cursor(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: pane::main::MainPaneOverlays<'_>,
    selection_frame: &mut SelectionFrame,
    reserve_collapsed_separator: bool,
) -> Option<Position> {
    let pane::main::MainPaneOverlays {
        help_visible,
        mut input_page,
        mut settings,
        mut login,
        approval,
        queue,
    } = overlays;
    // Page: one-column ordinary side margins (or the collapsed grip reserve),
    // capped at the configured max width and horizontally aligned
    // (居中/左对齐/右对齐); text wraps within this
    // content width. Resolved by `screen::main_page_rect` — the single owner
    // of the margin/cap/align policy — so rendering and the runtime scroll
    // path always see the same width.
    let page = screen::main_page_rect(area, state, reserve_collapsed_separator);
    let input_page_open = input_page.is_some() || settings.is_some() || login.is_some();
    let drafting = state.session.new_conversation.is_some();
    // Input Pages replace the input bar and take two thirds of the page height
    // without a floating window. The transcript keeps the top third.
    let bottom_rows = bottom_area_rows(
        area.height,
        page.width,
        input,
        input_page_open,
        state.config.user_input_padding,
    );
    let bottom = Constraint::Length(bottom_rows);
    let accessories = if drafting {
        Vec::new()
    } else {
        input_accessories(state, approval, queue)
    };
    let accessory_budget = area.height.saturating_sub(1 + bottom_rows + 3);
    let accessory_plan = allocate_accessories(&accessories, accessory_budget);
    let accessory_rows = |kind| {
        accessory_plan
            .iter()
            .find(|item| item.kind == kind)
            .map_or(0, |item| item.rows)
    };
    let approval_rows = accessory_rows(InputAccessoryKind::Approval);
    let goal_rows = accessory_rows(InputAccessoryKind::Goal);
    let plan_rows = accessory_rows(InputAccessoryKind::Plan);
    let todo_rows = accessory_rows(InputAccessoryKind::Todo);
    let queue_visible = usize::from(accessory_rows(InputAccessoryKind::Queue));
    if input_page_open {
        let chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(approval_rows),
            Constraint::Length(goal_rows),
            Constraint::Length(plan_rows),
            Constraint::Length(todo_rows),
            Constraint::Length(queue_visible as u16),
            bottom,
            Constraint::Length(1), // one-row gap between input and status bar
            Constraint::Length(1), // status bar
            Constraint::Length(1), // session title row at the very bottom
        ])
        .split(page);

        region::transcript::render(
            frame,
            chunks[0],
            state,
            scroll,
            theme,
            help_visible,
            selection_frame,
        );
        if approval_rows > 0 {
            if let Some(card) = approval {
                render_approval(frame, chunks[1], card, theme);
            }
        }
        if goal_rows > 0 {
            render_info_accessory(
                frame,
                chunks[2],
                "Goal",
                state.goal.as_deref().unwrap_or(""),
                theme,
            );
        }
        if plan_rows > 0 {
            render_info_accessory(
                frame,
                chunks[3],
                "Plan",
                state.plan_mode.as_deref().unwrap_or(""),
                theme,
            );
        }
        if todo_rows > 0 {
            render_todo(frame, chunks[4], &state.todos, theme);
        }
        if queue_visible > 0 {
            render_queue(frame, chunks[5], queue, queue_visible, theme);
        }
        let cursor_anchor = if let Some(page) = input_page.as_mut() {
            region::input_page::render(frame, chunks[6], page, &state.config, theme)
        } else if let Some(settings) = settings.as_mut() {
            render_settings(frame, chunks[6], settings, &state.config, theme);
            None
        } else if let Some(login) = login.as_mut() {
            render_login(frame, chunks[6], login, theme);
            None
        } else {
            region::composer::render(
                frame,
                chunks[6],
                input,
                theme,
                state.config.user_input_padding as u16,
            )
        };
        region::status::render(frame, chunks[8], state, scroll, theme);
        region::status::render_title(frame, chunks[9], state, theme);
        // Slash-command suggestions float above the input bar (last draw wins).
        if !input_page_open {
            if let Some(suggest) = input.suggest.as_ref() {
                render_suggest(frame, suggest, chunks[6], theme);
            }
        }
        return cursor_anchor;
    }

    // Ordinary mode: accessories, input bar, status and title are part of the
    // scrollable content. They are pinned at the screen bottom while following
    // the transcript, and move down/off-screen when the user scrolls back.
    let bottom_stack = usize::from(approval_rows)
        + usize::from(goal_rows)
        + usize::from(plan_rows)
        + usize::from(todo_rows)
        + queue_visible
        + usize::from(bottom_rows)
        + 3;
    let transcript_bottom = region::transcript::render_combined(
        frame,
        page,
        state,
        scroll,
        theme,
        help_visible,
        bottom_stack,
        selection_frame,
    );
    let mut cursor_anchor = None;
    let mut input_rect = None;
    let mut y = page.y + transcript_bottom as u16;
    let end_y = page.y + page.height;

    if approval_rows > 0 && y < end_y {
        let h = (end_y - y).min(approval_rows);
        if let Some(card) = approval {
            render_approval(
                frame,
                ratatui::layout::Rect::new(page.x, y, page.width, h),
                card,
                theme,
            );
        }
        y = y.saturating_add(approval_rows);
    }
    if goal_rows > 0 && y < end_y {
        let h = (end_y - y).min(goal_rows);
        render_info_accessory(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, h),
            "Goal",
            state.goal.as_deref().unwrap_or(""),
            theme,
        );
        y = y.saturating_add(goal_rows);
    }
    if plan_rows > 0 && y < end_y {
        let h = (end_y - y).min(plan_rows);
        render_info_accessory(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, h),
            "Plan",
            state.plan_mode.as_deref().unwrap_or(""),
            theme,
        );
        y = y.saturating_add(plan_rows);
    }
    if todo_rows > 0 && y < end_y {
        let h = (end_y - y).min(todo_rows);
        render_todo(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, h),
            &state.todos,
            theme,
        );
        y = y.saturating_add(todo_rows);
    }
    if queue_visible > 0 && y < end_y {
        let h = (end_y - y).min(queue_visible as u16);
        render_queue(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, h),
            queue,
            queue_visible,
            theme,
        );
        y = y.saturating_add(queue_visible as u16);
    }
    if y < end_y {
        let h = (end_y - y).min(bottom_rows);
        let rect = ratatui::layout::Rect::new(page.x, y, page.width, h);
        input_rect = Some(rect);
        cursor_anchor = region::composer::render(
            frame,
            rect,
            input,
            theme,
            state.config.user_input_padding as u16,
        );
        y = y.saturating_add(bottom_rows);
    }
    if y < end_y {
        y = y.saturating_add(1);
    }
    if y < end_y {
        region::status::render(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, 1),
            state,
            scroll,
            theme,
        );
        y = y.saturating_add(1);
    }
    if y < end_y {
        region::status::render_title(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, 1),
            state,
            theme,
        );
    }
    if let Some(suggest) = input.suggest.as_ref() {
        if let Some(rect) = input_rect {
            render_suggest(frame, suggest, rect, theme);
        }
    }
    cursor_anchor
}

#[cfg(test)]
mod main_pane_tests;

/// Display rows of the input buffer after width-aware wrapping, capped at
/// `INPUT_MAX_ROWS`. The box grows with wrapped content (a single long line
/// can occupy several rows), and `render_input`'s window scrolls within it
/// once the content exceeds the cap.
fn input_rows(input: &InputState, wrap_width: usize, padding: usize) -> usize {
    let inner = wrap_width.saturating_sub(padding.saturating_mul(2)).max(1);
    let display = input.display_text();
    display
        .text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                1
            } else {
                wrap_text(line, inner).len().max(1)
            }
        })
        .sum::<usize>()
        .max(1)
        .min(INPUT_MAX_ROWS)
}
