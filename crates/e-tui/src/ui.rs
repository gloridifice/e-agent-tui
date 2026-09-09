//! Ratatui rendering: status bar, transcript, and ruled input bar.

use ratatui::{
    layout::{Constraint, Layout, Position},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::TuiApp,
    cache::{MessageLineRange, TranscriptRenderCache},
    command_catalog::CommandSource,
    config::{Theme, ThinkingDisplayMode},
    display::{
        ActivityKind, ActivityRow, CardRole, ContentCard, DisplayItem, DisplayTone, ThinkingNode,
        TranscriptBlock, TranscriptFormat,
    },
    input::InputState,
    input_page::{FocusId, InputPage, InputPageSession, ModelPage, ResumePage, ThemePage},
    interaction::PendingPrompt,
    login::LoginState,
    mouse_selection::MouseSelection,
    projection::TranscriptNode,
    settings::SettingsState,
    transcript_layout::{truncate_activity_line, wrap_line, wrapped_rows, ProvenanceLayoutRow},
    SessionStatus as AgentStatus,
};
mod accessories;
/// Multiline input shows at most this many rows (D23).
pub mod component;
mod input;
mod layout;
mod overlay;
mod pages;
pub mod pane;
pub mod region;
pub mod screen;
pub(crate) mod selection;
mod status;
mod transcript;
use pages::trim_to_width;

pub use transcript::{provenance_layout_rows, scroll_lines, scroll_page};

pub use crate::interaction::ScrollState;

/// Guard against pathological transcripts (huge snapshots).
const MAX_RENDER_LINES_PER_MSG: usize = 800;

pub use screen::RenderOverlays;
pub use selection::{selection_context, Presentation};

#[cfg(test)]
pub(crate) use layout::{bottom_area_rows, input_rows, INPUT_MAX_ROWS};

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
    layout::main_page_rect(main, state, reserve_collapsed_separator).width
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
    input_page: Option<&InputPageSession>,
    approval: Option<&crate::interaction::ApprovalCard>,
    queue: &[PendingPrompt],
) -> usize {
    let plan = layout::BottomLayoutPlan::new(
        size.height,
        input_bar_width(size.width, state),
        state,
        input,
        input_page.map(pages::preferred_rows),
        approval,
        queue,
        state.session.new_conversation.is_some(),
    );
    usize::from(size.height)
        .saturating_sub(plan.bottom_stack)
        .max(1)
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
        &Presentation::default(),
    )
    .cursor
}

/// Ephemeral artifacts produced by one render attempt. The runner publishes
/// `presentation` only after terminal submission succeeds.
pub struct RenderOutput {
    pub cursor: Option<Position>,
    pub presentation: Presentation,
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
    committed: &Presentation,
) -> RenderOutput {
    let resizing = overlays.pane_resize.is_active();
    let context = selection_context(
        state,
        overlays.input_page.as_deref(),
        overlays.approval,
        overlays.help_visible,
    );
    if !resizing {
        if let Some(cursor) = committed.replay(frame.buffer_mut(), selection, context) {
            return RenderOutput {
                cursor,
                presentation: committed.clone(),
            };
        }
    }
    let toast = (!resizing).then_some(overlays.toast).flatten();
    let cursor = screen::render_with_cursor(frame, state, input, scroll, theme, overlays);
    if let Some(toast) = toast {
        overlay::render_toast(frame, toast, theme);
    }
    let pane_separator = match screen::layout(
        frame.area(),
        state.config.message_pane_percent,
        state.preview.fullscreen && state.history_page.is_none(),
    ) {
        screen::ScreenLayout::Split { main, .. } => Some(main.right()),
        _ => None,
    };
    let presentation = Presentation::capture(frame.buffer_mut(), cursor, context, !resizing)
        .with_pane_separator(pane_separator);
    if !resizing
        && presentation
            .selection_frame()
            .same_geometry(committed.selection_frame())
    {
        selection::paint(committed.selection_frame(), selection, frame.buffer_mut());
    }
    RenderOutput {
        cursor,
        presentation,
    }
}

#[cfg(test)]
mod main_pane_tests;
