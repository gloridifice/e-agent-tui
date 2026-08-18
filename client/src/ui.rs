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
    cache::MessageLineRange,
    command_catalog::CommandSource,
    config::{Theme, ThinkingDisplayMode},
    display::{
        allocate_accessories, ActivityRow, ActivityState, CardRole, ContentCard, DisplayItem,
        DisplayTone, InputAccessory, InputAccessoryKind, TranscriptBlock, TranscriptFormat,
    },
    input::{InputState, Suggestion, SuggestionKind},
    input_page::{FocusId, InputPage, InputPageSession, ModelPage, ResumePage, ThemePage},
    login::LoginState,
    model::{breathing_color, settle_color, AgentStatus, AppState},
    projection::TranscriptNode,
    settings::SettingsState,
    transcript_layout::{truncate_activity_line, wrap_line, wrapped_rows, CopyLayoutRow},
};
#[cfg(test)]
use crate::{
    display::{ActivityContinuation, DisplayId},
    model::{Msg, ToolState},
};

/// Multiline input shows at most this many rows (D23).
mod accessories;
mod input;
mod overlay;
mod pages;
mod status;
mod transcript;
use pages::{render_input_page, render_login, render_settings, trim_to_width, wrap_text};

use accessories::{
    render_approval, render_info_accessory, render_question, render_question_bar, render_queue,
    render_suggest, render_todo,
};
use input::render_input;
use overlay::help_overlay;
use status::{render_status, render_title};
pub use transcript::{copy_layout_rows, scroll_lines, scroll_page};
#[cfg(test)]
use transcript::{legacy_test_lines, legacy_test_styled_lines};
use transcript::{render_transcript, render_transcript_combined, InputPageRegions};

const INPUT_MAX_ROWS: usize = 3;

pub struct ScrollState {
    pub follow: bool,
    pub offset: usize,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            follow: true,
            offset: 0,
        }
    }
}

/// Copy-mode view state handed to the renderer.
pub struct CopyOverlay {
    /// Global row of the cursor.
    pub cursor_row: usize,
    /// Selected global row range (inclusive).
    pub sel: Option<(usize, usize)>,
}

/// Guard against pathological transcripts (huge snapshots).
const MAX_RENDER_LINES_PER_MSG: usize = 800;

/// Horizontal page margin in columns (user preference, 4 spaces each side).
const PAGE_MARGIN: u16 = 4;

/// Per-frame overlay/page handles handed to `render` as one context (the
/// surfaces are owned by the main loop; the renderer only borrows them for
/// one frame).
pub struct RenderOverlays<'a> {
    pub help_visible: bool,
    pub overlay: Option<&'a CopyOverlay>,
    pub toast: Option<&'a str>,
    /// Unified configuration page used by the live main loop.
    pub input_page: Option<&'a mut InputPageSession>,
    /// Legacy direct page hooks retained for focused renderer tests.
    pub settings: Option<&'a mut SettingsState>,
    pub login: Option<&'a mut LoginState>,
}

fn input_accessories(state: &AppState) -> Vec<InputAccessory> {
    let mut accessories = Vec::new();
    if state.question.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Question,
            priority: 100,
            desired_rows: 3,
            minimum_rows: 3,
            blocking: true,
            insertion_order: 0,
        });
    }
    if state.approval.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Approval,
            priority: 90,
            desired_rows: 3,
            minimum_rows: 3,
            blocking: true,
            insertion_order: 1,
        });
    }
    if state.goal.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Goal,
            priority: 40,
            desired_rows: 1,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 2,
        });
    }
    if state.plan_mode.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Plan,
            priority: 30,
            desired_rows: 1,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 3,
        });
    }
    if !state.todos.is_empty() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Todo,
            priority: 20,
            desired_rows: (state.todos.len() + 1).min(6) as u16,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 4,
        });
    }
    if !state.queue.is_empty() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Queue,
            priority: 10,
            desired_rows: state.queue.len().min(u16::MAX as usize) as u16,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 5,
        });
    }
    accessories
}

fn bottom_area_rows(area_height: u16, input: &InputState, input_page_open: bool) -> u16 {
    if input_page_open {
        ((area_height as u32) * 2 / 3).min(area_height.saturating_sub(3) as u32) as u16
    } else {
        (input_rows(input) + 2) as u16
    }
}

/// Visible transcript rows for the same bottom/accessory policy used by
/// `render`; keyboard and mouse scrolling must use this rather than the full
/// terminal height.
pub fn transcript_view_height(
    area_height: u16,
    state: &AppState,
    input: &InputState,
    input_page_open: bool,
) -> usize {
    let bottom_rows = bottom_area_rows(area_height, input, input_page_open);
    let accessory_budget = area_height.saturating_sub(1 + bottom_rows + 3);
    let accessory_rows: u16 = allocate_accessories(&input_accessories(state), accessory_budget)
        .iter()
        .map(|item| item.rows)
        .sum();
    usize::from(
        area_height
            .saturating_sub(bottom_rows + accessory_rows + 3)
            .max(1),
    )
}

pub fn render(
    frame: &mut Frame,
    state: &mut AppState,
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
    state: &mut AppState,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: RenderOverlays<'_>,
) -> Option<Position> {
    let RenderOverlays {
        help_visible,
        overlay,
        toast,
        mut input_page,
        mut settings,
        mut login,
    } = overlays;
    let area = frame.area();
    // Page: fixed side margins, capped at the configured max width and
    // centered; text wraps within this content width.
    let available = area.width.saturating_sub(PAGE_MARGIN * 2);
    let max_width = state.config.page_max_width as u16;
    let content_width = if max_width > 0 {
        max_width.min(available)
    } else {
        available
    };
    let page = ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(content_width) / 2,
        y: area.y,
        width: content_width,
        height: area.height,
    };
    let input_page_open = input_page.is_some() || settings.is_some() || login.is_some();
    let drafting = state.new_conversation.is_some();
    // Input Pages replace the input bar and take two thirds of the page height
    // without a floating window. The transcript keeps the top third.
    let bottom_rows = bottom_area_rows(area.height, input, input_page_open);
    let bottom = Constraint::Length(bottom_rows);
    let accessories = if drafting {
        Vec::new()
    } else {
        input_accessories(state)
    };
    let accessory_budget = area.height.saturating_sub(1 + bottom_rows + 3);
    let accessory_plan = allocate_accessories(&accessories, accessory_budget);
    let accessory_rows = |kind| {
        accessory_plan
            .iter()
            .find(|item| item.kind == kind)
            .map_or(0, |item| item.rows)
    };
    let question_rows = accessory_rows(InputAccessoryKind::Question);
    let approval_rows = accessory_rows(InputAccessoryKind::Approval);
    let goal_rows = accessory_rows(InputAccessoryKind::Goal);
    let plan_rows = accessory_rows(InputAccessoryKind::Plan);
    let todo_rows = accessory_rows(InputAccessoryKind::Todo);
    let queue_visible = usize::from(accessory_rows(InputAccessoryKind::Queue));
    if input_page_open {
        let chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(question_rows),
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

        render_transcript(
            frame,
            chunks[0],
            state,
            scroll,
            theme,
            help_visible,
            overlay,
        );
        if !drafting {
            if let Some(question) = state.question.as_ref() {
                render_question(frame, chunks[1], question, theme);
            }
        }
        if approval_rows > 0 {
            render_approval(frame, chunks[2], state.approval.as_ref().unwrap(), theme);
        }
        if goal_rows > 0 {
            render_info_accessory(
                frame,
                chunks[3],
                "Goal",
                state.goal.as_deref().unwrap_or(""),
                theme,
            );
        }
        if plan_rows > 0 {
            render_info_accessory(
                frame,
                chunks[4],
                "Plan",
                state.plan_mode.as_deref().unwrap_or(""),
                theme,
            );
        }
        if todo_rows > 0 {
            render_todo(frame, chunks[5], &state.todos, theme);
        }
        if queue_visible > 0 {
            render_queue(frame, chunks[6], &state.queue, queue_visible, theme);
        }
        let cursor_anchor = if let Some(page) = input_page.as_mut() {
            render_input_page(frame, chunks[7], page, &state.config, theme);
            None
        } else if let Some(settings) = settings.as_mut() {
            render_settings(frame, chunks[7], settings, &state.config, theme);
            None
        } else if let Some(login) = login.as_mut() {
            render_login(frame, chunks[7], login, theme);
            None
        } else if let Some(question) = state.question.as_ref().filter(|_| !drafting) {
            // The input bar becomes the selection bar while a question pends.
            render_question_bar(
                frame,
                chunks[7],
                question,
                theme,
                state.config.user_input_padding as u16,
            )
        } else {
            render_input(
                frame,
                chunks[7],
                input,
                theme,
                overlay.is_some(),
                toast,
                state.config.user_input_padding as u16,
            )
        };
        render_status(frame, chunks[9], state, scroll, theme);
        render_title(frame, chunks[10], state, theme);
        // Slash-command suggestions float above the input bar (last draw wins).
        if !input_page_open && (drafting || state.question.is_none()) {
            if let Some(suggest) = input.suggest.as_ref() {
                render_suggest(frame, suggest, chunks[7], theme);
            }
        }
        return cursor_anchor;
    }

    // Ordinary mode: accessories, input bar, status and title are part of the
    // scrollable content. They are pinned at the screen bottom while following
    // the transcript, and move down/off-screen when the user scrolls back.
    let bottom_stack = usize::from(question_rows)
        + usize::from(approval_rows)
        + usize::from(goal_rows)
        + usize::from(plan_rows)
        + usize::from(todo_rows)
        + queue_visible
        + usize::from(bottom_rows)
        + 3;
    let transcript_bottom = render_transcript_combined(
        frame,
        page,
        state,
        scroll,
        theme,
        help_visible,
        overlay,
        bottom_stack,
    );
    let mut cursor_anchor = None;
    let mut input_rect = None;
    let mut y = page.y + transcript_bottom as u16;
    let end_y = page.y + page.height;

    if question_rows > 0 && y < end_y {
        let h = (end_y - y).min(question_rows);
        render_question(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, h),
            state.question.as_ref().unwrap(),
            theme,
        );
        y = y.saturating_add(question_rows);
    }
    if approval_rows > 0 && y < end_y {
        let h = (end_y - y).min(approval_rows);
        render_approval(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, h),
            state.approval.as_ref().unwrap(),
            theme,
        );
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
            &state.queue,
            queue_visible,
            theme,
        );
        y = y.saturating_add(queue_visible as u16);
    }
    if y < end_y {
        let h = (end_y - y).min(bottom_rows);
        let rect = ratatui::layout::Rect::new(page.x, y, page.width, h);
        input_rect = Some(rect);
        cursor_anchor = if let Some(question) = state.question.as_ref().filter(|_| !drafting) {
            render_question_bar(
                frame,
                rect,
                question,
                theme,
                state.config.user_input_padding as u16,
            )
        } else {
            render_input(
                frame,
                rect,
                input,
                theme,
                overlay.is_some(),
                toast,
                state.config.user_input_padding as u16,
            )
        };
        y = y.saturating_add(bottom_rows);
    }
    if y < end_y {
        y = y.saturating_add(1);
    }
    if y < end_y {
        render_status(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, 1),
            state,
            scroll,
            theme,
        );
        y = y.saturating_add(1);
    }
    if y < end_y {
        render_title(
            frame,
            ratatui::layout::Rect::new(page.x, y, page.width, 1),
            state,
            theme,
        );
    }
    if drafting || state.question.is_none() {
        if let Some(suggest) = input.suggest.as_ref() {
            if let Some(rect) = input_rect {
                render_suggest(frame, suggest, rect, theme);
            }
        }
    }
    cursor_anchor
}

fn input_rows(input: &InputState) -> usize {
    if !input.multiline {
        1
    } else {
        // Display lines = newline count + 1. `str::lines()` undercounts a
        // trailing newline ("a\n" renders two rows but lines() reports one),
        // which kept the box from growing on Shift+Enter before any text.
        (input.buf.matches('\n').count() + 1).min(INPUT_MAX_ROWS)
    }
}

/// Status strip: below the input bar, no background, dim text.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppState, Msg};
    use crate::render::RenderLine;
    use ratatui::backend::Backend;

    struct CursorTrackingBackend {
        inner: ratatui::backend::TestBackend,
        visible: bool,
        show_calls: usize,
        draw_while_visible: bool,
    }

    impl CursorTrackingBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                inner: ratatui::backend::TestBackend::new(width, height),
                visible: false,
                show_calls: 0,
                draw_while_visible: false,
            }
        }
    }

    impl Backend for CursorTrackingBackend {
        fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
        {
            self.draw_while_visible |= self.visible;
            self.inner.draw(content)
        }

        fn hide_cursor(&mut self) -> std::io::Result<()> {
            self.visible = false;
            self.inner.hide_cursor()
        }

        fn show_cursor(&mut self) -> std::io::Result<()> {
            self.visible = true;
            self.show_calls += 1;
            self.inner.show_cursor()
        }

        fn get_cursor_position(&mut self) -> std::io::Result<Position> {
            self.inner.get_cursor_position()
        }

        fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> std::io::Result<()> {
            self.inner.set_cursor_position(position)
        }

        fn clear(&mut self) -> std::io::Result<()> {
            self.inner.clear()
        }

        fn size(&self) -> std::io::Result<ratatui::layout::Size> {
            self.inner.size()
        }

        fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
            self.inner.window_size()
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    fn state_with(msgs: Vec<Msg>) -> AppState {
        let mut s = AppState::default();
        s.msgs = msgs;
        s
    }

    #[test]
    fn reasoning_blocks_are_hidden_but_cards_keep_copy_provenance() {
        let mut s = AppState::default();
        s.units.insert(10, "reason source".into());
        s.units.insert(11, "card source".into());
        s.msgs.push(Msg::Block(TranscriptBlock {
            id: DisplayId::event(1, "reasoning"),
            unit: Some(10),
            content: "reason source".into(),
            format: TranscriptFormat::Reasoning,
            tone: DisplayTone::Dim,
            copy_source: "reason source".into(),
            streaming: false,
        }));
        s.msgs.push(Msg::Card(ContentCard {
            id: DisplayId::event(2, "context"),
            unit: Some(11),
            header: Some("Context".into()),
            content: "card source".into(),
            role: CardRole::Context,
            tone: DisplayTone::Dim,
            horizontal_padding: 2,
            copy_source: "card source".into(),
        }));
        // Thinking output never renders to the transcript.
        assert!(
            legacy_test_styled_lines(&s.msgs[0], &s, 80).is_empty(),
            "reasoning block renders nothing"
        );
        let card_lines = legacy_test_styled_lines(&s.msgs[1], &s, 20);
        assert!(card_lines.len() >= 4);
        assert!(card_lines.iter().all(|line| line.width() == 20));
        let rows = copy_layout_rows(&s);
        // Hidden reasoning is not copyable; the visible card still is.
        assert!(!rows.iter().any(|row| row.unit == 10));
        assert!(rows.iter().any(|row| row.unit == 11));
    }

    #[test]
    fn context_cards_cap_wrapped_content_at_five_rows_with_ellipsis() {
        let mut s = AppState::default();
        let source = "abcdefghijklmnopqrstuv";
        s.units.insert(12, source.into());
        s.msgs.push(Msg::Card(ContentCard {
            id: DisplayId::event(3, "context"),
            unit: Some(12),
            header: Some("Context · instructions".into()),
            content: source.into(),
            role: CardRole::Context,
            tone: DisplayTone::Dim,
            horizontal_padding: 2,
            copy_source: source.into(),
        }));

        // The 24-character source wraps into six rows at the four-column
        // content width. Only four source rows and the truncation marker show.
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 6);
        let text = |line: &Line<'_>| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim()
                .to_owned()
        };
        assert_eq!(lines.len(), 8, "top + header + five content + bottom");
        assert_eq!(
            lines[2..7].iter().map(text).collect::<Vec<_>>(),
            ["abcd", "efgh", "ijkl", "mnop", "..."]
        );
        assert_eq!(s.units[&12], source, "copy source remains unabridged");
    }

    #[test]
    fn reasoning_display_modes_render_compact_lines_and_full() {
        let mut s = AppState::default();
        s.transcript_cache.width = 80;
        s.units.insert(10, "one\ntwo\nthree".into());
        s.msgs.push(Msg::Block(TranscriptBlock {
            id: DisplayId::event(1, "reasoning"),
            unit: Some(10),
            content: "one\ntwo\nthree".into(),
            format: TranscriptFormat::Reasoning,
            tone: DisplayTone::Dim,
            copy_source: "one\ntwo\nthree".into(),
            streaming: false,
        }));

        // Compact (the default) renders only the Thinking indicator row.
        assert!(legacy_test_styled_lines(&s.msgs[0], &s, 80).is_empty());
        assert!(!copy_layout_rows(&s).iter().any(|row| row.unit == 10));

        // Lines shows at most the configured first N reasoning lines.
        s.config.thinking_display = "lines".into();
        s.config.thinking_lines = 2;
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 80);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "one");
        assert_eq!(lines[1].spans[0].content, "two");
        assert_eq!(
            copy_layout_rows(&s)
                .iter()
                .filter(|row| row.unit == 10)
                .count(),
            2,
            "only the visible first lines enter copy provenance"
        );

        // Full shows the complete reasoning text.
        s.config.thinking_display = "full".into();
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 80);
        assert_eq!(lines.len(), 3);
        assert_eq!(
            copy_layout_rows(&s)
                .iter()
                .filter(|row| row.unit == 10)
                .count(),
            3
        );
    }

    /// `Lines` mode caps reasoning to the configured number of DISPLAY rows:
    /// width-aware wrapping happens before the cap, so one long source line
    /// can consume the whole budget and push later lines out entirely.
    #[test]
    fn lines_mode_caps_reasoning_to_wrapped_display_rows() {
        use ratatui::backend::TestBackend;
        let mut state = AppState::default();
        state.config.thinking_display = "lines".into();
        state.config.thinking_lines = 2;
        state.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 1,
            "data": {"chunk": {"type": "reasoning-delta", "text": "aaaaaaaaaaaaaaaaaaaaaa\nbb\ncc"}, "turn": 1, "step": 0}
        }));
        let mut scroll = ScrollState::default();
        let theme = state.theme();
        let backend = TestBackend::new(10, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 10, 20);
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        let rows: Vec<String> = state
            .transcript_cache
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        // 22 'a's wrap into 3 rows at width 10; the 2-row cap keeps only the
        // first two, and "bb"/"cc" never render.
        let content: Vec<&String> = rows.iter().filter(|r| !r.is_empty()).collect();
        assert_eq!(content.len(), 2, "{rows:?}");
        assert_eq!(content[0], &"a".repeat(10));
        assert_eq!(content[1], &"a".repeat(10));
    }

    /// In `Lines`/`Full` modes the visible reasoning content supersedes the
    /// breathing `Thinking...` indicator: no indicator row, no gap row, only
    /// the content. `Compact` keeps the old fold.
    #[test]
    fn visible_reasoning_supersedes_the_thinking_indicator() {
        use ratatui::backend::TestBackend;
        for mode in ["lines", "full"] {
            let mut state = AppState::default();
            state.config.thinking_display = mode.into();
            state.start_thinking();
            state.apply_event(&serde_json::json!({
                "type": "assistant/chunk", "seq": 2,
                "data": {"chunk": {"type": "reasoning-delta", "text": "thought"}, "turn": 1, "step": 0}
            }));
            let mut scroll = ScrollState::default();
            let theme = state.theme();
            let backend = TestBackend::new(40, 12);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let area = ratatui::layout::Rect::new(0, 0, 40, 10);
            terminal
                .draw(|frame| {
                    render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
                })
                .unwrap();
            let rows: Vec<String> = state
                .transcript_cache
                .lines
                .iter()
                .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
                .collect();
            assert!(
                !rows.iter().any(|r| r.contains("Thinking")),
                "{mode}: indicator must be superseded: {rows:?}"
            );
            assert_eq!(
                rows.first().map(|r| r.trim_end()),
                Some("thought"),
                "{mode}: {rows:?}"
            );
        }
        // Compact still folds the content into the indicator.
        let mut state = AppState::default();
        state.start_thinking();
        state.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 2,
            "data": {"chunk": {"type": "reasoning-delta", "text": "thought"}, "turn": 1, "step": 0}
        }));
        let mut scroll = ScrollState::default();
        let theme = state.theme();
        let backend = TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 40, 10);
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        let rows: Vec<String> = state
            .transcript_cache
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(rows.iter().any(|r| r.contains("Thinking")), "{rows:?}");
        assert!(!rows.iter().any(|r| r.contains("thought")), "{rows:?}");
    }

    /// Without reasoning content the indicator keeps its normal place even in
    /// `Lines`/`Full` (here an answer block follows it).
    #[test]
    fn thinking_indicator_stays_before_plain_answer_in_full_mode() {
        use ratatui::backend::TestBackend;
        let mut state = AppState::default();
        state.config.thinking_display = "full".into();
        state.start_thinking();
        state.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 2,
            "data": {"chunk": {"type": "text-delta", "text": "hello"}, "turn": 1, "step": 0}
        }));
        let mut scroll = ScrollState::default();
        let theme = state.theme();
        let backend = TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 40, 10);
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        let rows: Vec<String> = state
            .transcript_cache
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(rows.iter().any(|r| r.contains("Thinking")), "{rows:?}");
        assert!(rows.iter().any(|r| r.contains("hello")), "{rows:?}");
    }

    /// A superseded (hidden) Thinking row must not poison the animation
    /// patch loop: ticks mark it dirty, but hidden rows patch nothing and do
    /// not trigger a full cache rebuild.
    #[test]
    fn hidden_thinking_row_patches_do_not_force_rebuilds() {
        use ratatui::backend::TestBackend;
        let mut state = AppState::default();
        state.config.thinking_display = "lines".into();
        state.start_thinking();
        state.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 2,
            "data": {"chunk": {"type": "reasoning-delta", "text": "thought"}, "turn": 1, "step": 0}
        }));
        let mut scroll = ScrollState::default();
        let theme = state.theme();
        let backend = TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 40, 10);
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        state.transcript_cache.take_work_stats();
        assert!(crate::model::tick_spinners(
            &mut state,
            std::time::Instant::now()
        ));
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        let work = state.transcript_cache.take_work_stats();
        assert_eq!(
            work.rebuilds, 0,
            "hidden thinking row must not force rebuilds"
        );
    }

    /// Legacy copy provenance agrees with the supersede rule: the hidden
    /// Thinking row contributes no rows, so the reasoning content starts at
    /// the very first display row in `Full` mode.
    #[test]
    fn legacy_copy_rows_skip_superseded_thinking_indicator() {
        let mut s = AppState::default();
        s.transcript_cache.width = 80;
        s.config.thinking_display = "full".into();
        s.units.insert(10, "thought".into());
        s.msgs.push(Msg::Thinking(crate::model::ThinkingCard {
            state: crate::model::ThinkState::Done,
            count: 1,
            done_since: None,
            done_from: None,
        }));
        s.msgs.push(Msg::Block(TranscriptBlock {
            id: DisplayId::event(1, "reasoning"),
            unit: Some(10),
            content: "thought".into(),
            format: TranscriptFormat::Reasoning,
            tone: DisplayTone::Dim,
            copy_source: "thought".into(),
            streaming: false,
        }));
        let rows = copy_layout_rows(&s);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            rows[0].global_row, 0,
            "no leading gap for a superseded indicator"
        );
        // Compact: the indicator is visible again and the reasoning block
        // renders nothing.
        s.config.thinking_display = "compact".into();
        let rows = copy_layout_rows(&s);
        assert!(rows.is_empty(), "hidden reasoning contributes no copy rows");
    }

    #[test]
    fn user_block_has_vertical_padding() {
        let s = state_with(vec![Msg::User {
            text: "你好".into(),
        }]);
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 80);
        // 1 padding row above + 1 content row + 1 padding row below.
        assert_eq!(lines.len(), 3, "user block = padding + content + padding");
        assert_eq!(lines[0].width(), 80, "top padding row fills the width");
        assert_eq!(lines[2].width(), 80, "bottom padding row fills the width");
        assert_eq!(
            lines[1].spans[0].content, "  ",
            "content row leads with the default 2-column gutter"
        );
        assert_eq!(lines[1].spans[1].content, "你好", "text follows the gutter");
    }

    #[test]
    fn user_block_gutter_follows_config() {
        let mut s = state_with(vec![Msg::User { text: "hi".into() }]);
        s.config.user_input_padding = 6;
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 80);
        assert_eq!(
            lines[1].spans[0].content, "      ",
            "gutter width comes from the config"
        );
    }

    #[test]
    fn user_block_multiline_counts_extra_rows() {
        let s = state_with(vec![Msg::User {
            text: "a\nb\nc".into(),
        }]);
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 80);
        assert_eq!(lines.len(), 5);
    }

    /// Wrapped continuation rows keep the configured horizontal gutter.
    #[test]
    fn user_block_wrapped_rows_keep_gutter() {
        let s = state_with(vec![Msg::User {
            text: "x".repeat(30),
        }]);
        // Width 12, gutter 2 → three wrapped rows of 10 chars each.
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 12);
        assert_eq!(lines.len(), 5, "padding + 3 wrapped rows + padding");
        for line in &lines[1..4] {
            assert_eq!(
                line.spans[0].content, "  ",
                "every wrapped row leads with the gutter"
            );
            assert_eq!(
                line.spans[1].content, "xxxxxxxxxx",
                "wrapped row carries 10 columns of text"
            );
        }
    }

    #[test]
    fn assistant_lines_have_no_side_bar() {
        let rl = RenderLine {
            line: Line::from("普通文本"),
            unit: 0,
            raw_line: Some(0),
            atomic: false,
            fill: false,
        };
        let s = state_with(vec![Msg::Assistant {
            text: "普通文本".into(),
            lines: vec![rl],
            unit_start: 0,
        }]);
        let lines = legacy_test_lines(&s.msgs[0], &s);
        assert_ne!(lines[0].spans[0].content, "▎", "no green bar prefix");
    }

    #[test]
    fn atomic_rows_keep_frames() {
        let rl = RenderLine {
            line: Line::from("┌────┐"),
            unit: 0,
            raw_line: None,
            atomic: true,
            fill: false,
        };
        let s = state_with(vec![Msg::Assistant {
            text: "t".into(),
            lines: vec![rl],
            unit_start: 0,
        }]);
        let lines = legacy_test_lines(&s.msgs[0], &s);
        assert_eq!(lines[0].spans[0].content, "┌────┐", "table frame untouched");
    }

    /// File group format: `read a, b; edit c` on one line; while the edit is
    /// pending the read part keeps its own `;`-terminated line and the
    /// running edit gets a spinner line.
    #[test]
    fn file_group_renders_one_line_format() {
        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        for (seq, id, name, path) in [
            (1, "r1", "read", "src/input.rs"),
            (2, "r2", "read", "src/foo.rs"),
            (3, "e1", "edit", "src/ui.rs"),
        ] {
            s.apply_event(&serde_json::json!({
                "type": "tool/call", "seq": seq, "time": 0,
                "data": {
                    "callId": id, "name": name,
                    "arguments": format!("{{\"file_path\": \"{path}\"}}")
                }
            }));
        }
        // Settle the reads → edit pending: read line with trailing ";",
        // spinner edit line.
        for (seq, id) in [(4u64, "r1"), (5, "r2")] {
            s.apply_event(&serde_json::json!({
                "type": "tool/result", "seq": seq, "time": 0,
                "data": {"message": {"content": [{
                    "type": "tool-result", "toolCallId": id,
                    "content": [{"type": "text", "text": "ok"}]
                }]}}
            }));
        }
        let lines = legacy_test_lines(&s.msgs[0], &s);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|sp| sp.content.as_ref()).collect())
            .collect();
        assert_eq!(text[0], "  • read input.rs, foo.rs;");
        assert!(text[1].contains("edit ui.rs"), "got: {text:?}");
        // Settle the edit → one line, no trailing semicolon.
        s.apply_event(&serde_json::json!({
            "type": "tool/result", "seq": 6, "time": 0,
            "data": {"message": {"content": [{
                "type": "tool-result", "toolCallId": "e1",
                "content": [{"type": "text", "text": "ok"}]
            }]}}
        }));
        let lines = legacy_test_lines(&s.msgs[0], &s);
        assert_eq!(lines.len(), 1);
        let text: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert_eq!(text, "  • read input.rs, foo.rs; edit ui.rs");
    }

    #[test]
    fn editor_create_is_a_concise_standalone_relative_path_row() {
        let mut s = AppState::default();
        s.session_cwd = Some(r"G:\workspace".into());
        s.apply_event(&serde_json::json!({
            "type": "tool/call", "seq": 1, "time": 0,
            "data": {
                "callId": "c1", "name": "str_replace_editor",
                "arguments": serde_json::json!({
                    "command": "create", "path": r"G:\workspace\src\new.rs"
                }).to_string()
            }
        }));
        let running: String = legacy_test_lines(&s.msgs[0], &s)[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(
            running.contains("create src/new.rs"),
            "running row: {running}"
        );

        s.apply_event(&serde_json::json!({
            "type": "tool/result", "seq": 2, "time": 1,
            "data": {"message": {"content": [{
                "type": "tool-result", "toolCallId": "c1",
                "content": [{"type": "text", "text": "created"}]
            }]}}
        }));
        let done: String = legacy_test_lines(&s.msgs[0], &s)[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(done, "  • create src/new.rs");
    }

    /// Tool summaries and read/edit file lists use bark (#6f5d63, the `dim`
    /// theme slot) instead of the default white text color.
    #[test]
    fn activity_text_uses_bark_dim() {
        use crate::model::{FileAction, FileGroup, FileItem, ToolCard, ToolState};

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.msgs.push(Msg::Tool(ToolCard {
            call_id: "b1".into(),
            name: "bash".into(),
            summary: "cmd".into(),
            state: ToolState::Running,
            frame: 0,
            start_ms: 0,
            done_since: None,
            done_from: None,
        }));
        let theme = Theme::ferra();
        let lines = legacy_test_lines(&s.msgs[0], &s);
        let name = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "bash")
            .expect("tool name span");
        assert_eq!(
            name.style.fg,
            Some(theme.selection),
            "tool name uses umber (selection)"
        );
        let summary = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "cmd")
            .expect("summary span");
        assert_eq!(
            summary.style.fg,
            Some(theme.dim),
            "tool summary uses bark (dim)"
        );

        let mut s2 = AppState::default();
        s2.config = config;
        s2.msgs.push(Msg::FileGroup(FileGroup {
            items: vec![
                FileItem {
                    action: FileAction::Read,
                    call_id: "r1".into(),
                    file: "src/a.rs".into(),
                    ok: Some(true),
                },
                FileItem {
                    action: FileAction::Edit,
                    call_id: "e1".into(),
                    file: "src/b.rs".into(),
                    ok: Some(true),
                },
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        }));
        let lines = legacy_test_lines(&s2.msgs[0], &s2);
        let label = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "read")
            .expect("read label span");
        assert_eq!(
            label.style.fg,
            Some(theme.selection),
            "read/edit labels use umber (selection)"
        );
        let file = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "a.rs")
            .expect("file list span");
        assert_eq!(file.style.fg, Some(theme.dim), "file list uses bark (dim)");
        // Finished read/edit line leads with a green bullet on success.
        let bullet = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "•")
            .expect("bullet span");
        assert_eq!(bullet.style.fg, Some(theme.ok), "success bullet uses green");

        // Failure: bullet turns red (and only then).
        let mut s3 = AppState::default();
        let mut fail_cfg = crate::config::Config::default();
        fail_cfg.resolved_theme = Theme::ferra();
        s3.config = fail_cfg;
        s3.msgs.push(Msg::FileGroup(FileGroup {
            items: vec![FileItem {
                action: FileAction::Read,
                call_id: "r2".into(),
                file: "src/x.rs".into(),
                ok: Some(false),
            }],
            frame: 0,
            done_since: None,
            done_from: None,
        }));
        let lines = legacy_test_lines(&s3.msgs[0], &s3);
        let bullet = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "•")
            .expect("bullet span");
        assert_eq!(bullet.style.fg, Some(theme.err), "failure bullet uses red");
    }

    /// Done tool cards use a `• ` bullet (space provided by the separator
    /// span); the success/failure colors stay ok/err.
    #[test]
    fn tool_done_glyph_is_bullet() {
        use crate::model::{ToolCard, ToolState};

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config;
        s.msgs.push(Msg::Tool(ToolCard {
            call_id: "b1".into(),
            name: "pwsh".into(),
            summary: "cmd".into(),
            state: ToolState::Done {
                ok: true,
                lines: 3,
                lines_truncated: false,
                duration_ms: 0,
            },
            frame: 0,
            start_ms: 0,
            done_since: None,
            done_from: None,
        }));
        let theme = Theme::ferra();
        let lines = legacy_test_lines(&s.msgs[0], &s);
        let text: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(
            text.starts_with("  • "),
            "done glyph is a spaced bullet: {text}"
        );
        assert!(!text.contains('✓') && !text.contains('✗'));
        let bullet = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "•")
            .expect("bullet span");
        assert_eq!(bullet.style.fg, Some(theme.ok), "success keeps green");
    }

    /// The status bar sits one row below the input bar and shows a working
    /// bullet before the state label: gray while idle, breathing while the
    /// agent works.
    #[test]
    fn status_bar_has_spacing_and_working_bullet() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut cursor_anchor = None;
        terminal
            .draw(|f| {
                cursor_anchor = render_with_cursor(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        assert_eq!(
            cursor_anchor,
            Some(Position::new(6, 35)),
            "normal frame returns the hidden IME anchor"
        );
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(0, 0),
            "rendering does not expose or move the hardware cursor"
        );
        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0..80)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        // Layout: transcript 34, input 3 (y 34..37), spacer y=37, status
        // y=38, title row y=39.
        let spacer = row(37);
        assert!(
            spacer.trim().is_empty(),
            "one-row gap above the status bar: {spacer:?}"
        );
        let status = row(38);
        assert!(status.contains('•'), "working bullet present: {status:?}");
        assert!(
            !status.contains("idle") && !status.contains("running"),
            "no idle/running text label: {status:?}"
        );
        assert!(
            status.trim_start().starts_with("• standard"),
            "status shows indicator and mode: {status:?}"
        );
        assert!(
            !status.contains('—') && !status.contains("CH"),
            "missing model and usage are omitted: {status:?}"
        );
        assert!(status.contains("^h Help"), "right help hint: {status:?}");
        // Char index (not byte index — the row holds multi-byte `·`): each
        // cell contributes exactly one char, so this is also the cell column.
        let bullet_x = status.chars().position(|c| c == '•').expect("bullet cell");
        assert_eq!(bullet_x, 4, "bullet at the content area's leading edge");
        assert_eq!(
            buf[(bullet_x as u16, 38)].fg,
            theme.dim,
            "idle bullet is gray"
        );
        assert_eq!(
            buf[(bullet_x as u16, 38)].bg,
            ratatui::style::Color::Reset,
            "status rows do not paint a background"
        );
        // Running (with no visible activity and `working` left false): the
        // bullet still leaves gray (breathing toward yellow) — the running
        // status alone must drive the breath.
        s.status = AgentStatus::Running;
        s.model = Some("deepseek-chat".into());
        s.token_usage.input_tokens = 50;
        s.token_usage.cache_read_tokens = 50;
        s.activity_epoch = Some(
            std::time::Instant::now()
                - std::time::Duration::from_millis((crate::model::BREATH_CYCLE_MS / 2) as u64),
        );
        terminal
            .draw(|f| {
                cursor_anchor = render_with_cursor(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(0, 0),
            "animated status redraw never moves a visible hardware cursor"
        );
        let running_buf = terminal.backend().buffer();
        let running_status: String = (0..80)
            .map(|x| running_buf[(x, 38)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            running_status.contains("standard deepseek-chat CH50%"),
            "available model and cache rate are shown: {running_status:?}"
        );
        let backend_fg = terminal.backend().buffer()[(bullet_x as u16, 38)].fg;
        assert_ne!(backend_fg, theme.dim, "running bullet breathes (not gray)");
    }

    #[test]
    fn running_redraw_keeps_hardware_cursor_hidden_and_only_moves_ime_anchor() {
        let mut config = crate::config::Config::default();
        config.resolved_theme = Theme::ferra();
        let mut state = AppState::default();
        state.config = config.clone();
        state.status = AgentStatus::Running;
        state.activity_epoch = Some(std::time::Instant::now());
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = CursorTrackingBackend::new(80, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.hide_cursor().unwrap();

        let mut expected_anchor = None;
        for _ in 0..2 {
            terminal
                .draw(|frame| {
                    expected_anchor = render_with_cursor(
                        frame,
                        &mut state,
                        &input,
                        &mut scroll,
                        &theme,
                        RenderOverlays {
                            input_page: None,
                            help_visible: false,
                            overlay: None,
                            toast: None,
                            settings: None,
                            login: None,
                        },
                    );
                })
                .unwrap();
            terminal
                .set_cursor_position(expected_anchor.expect("input IME anchor"))
                .unwrap();
        }

        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            expected_anchor.unwrap(),
            "hidden terminal cursor stays anchored to the input"
        );
        assert_eq!(
            terminal.backend().show_calls,
            0,
            "no frame may show the hardware cursor"
        );
        assert!(
            !terminal.backend().draw_while_visible,
            "diff writer must never run while the hardware cursor is visible"
        );
        assert!(
            !terminal.backend().visible,
            "hardware cursor remains hidden"
        );
    }

    /// The title row below the status bar shows the session's latest title
    /// and falls back to “新会话” when no title exists.
    #[test]
    fn title_row_renders_below_status_bar() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.session_title = Some("重构 bridge".into());
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        // Fixed bottom: input(3, y18..21) + gap(y21) + status(y22) + title(y23).
        let title_row = |buf: &ratatui::buffer::Buffer, y: u16| -> String {
            (0u16..80)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        let title = title_row(terminal.backend().buffer(), 23);
        assert!(
            title.replace(' ', "").contains("重构bridge"),
            "title row: {title}"
        );
        assert!(
            title_row(terminal.backend().buffer(), 22).contains("• standard"),
            "status bar still one row above the title"
        );
        assert_eq!(
            terminal.backend().buffer()[(4, 23)].bg,
            ratatui::style::Color::Reset,
            "title row does not paint a background"
        );
        // Without a title the explicit fallback remains visible.
        s.session_title = None;
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let fallback = title_row(terminal.backend().buffer(), 23);
        assert!(
            fallback.replace(' ', "").contains("新会话"),
            "fallback title row: {fallback:?}"
        );

        // `/new` is a presentation-only draft: it hides the retained real
        // transcript and uses “新对话” without writing a session title.
        s.push_error_message("retained old transcript");
        s.begin_new_conversation("code");
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let draft_title = title_row(terminal.backend().buffer(), 23);
        assert!(
            draft_title.replace(' ', "").contains("新对话"),
            "draft title row: {draft_title:?}"
        );
        let screen = (0u16..24)
            .map(|y| title_row(terminal.backend().buffer(), y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!screen.contains("retained old transcript"), "{screen}");
        assert!(title_row(terminal.backend().buffer(), 22).contains("• code"));
        assert!(copy_layout_rows(&s).is_empty());
    }

    /// The title row also carries the workspace path, right-aligned; a long
    /// title truncates with an ellipsis so the path never scrolls off.
    #[test]
    fn title_row_shows_path_right_aligned_and_truncates_long_titles() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.session_title = Some("重构 bridge".into());
        s.session_cwd = Some(r"D:\MyProjects\Chore\dsh".into());
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let title = (0u16..80)
            .map(|x| buf[(x, 23)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>();
        // Left-aligned title; CJK wide glyphs occupy two cells, so strip
        // continuation cells and page-margin spaces before matching.
        assert!(
            title.replace(' ', "").starts_with("重构bridge"),
            "title left: {title:?}"
        );
        // Right-aligned path (all ASCII) sits flush against the content
        // area's right edge. The page leaves a 4-column margin on each side
        // of an 80-col terminal, so the 72-wide content area ends at x=75.
        assert!(
            title.trim_end().ends_with(r"D:\MyProjects\Chore\dsh"),
            "path right: {title:?}"
        );
        assert_eq!(
            buf[(75, 23)].symbol(),
            "h",
            "path ends at the content's right edge"
        );

        // A very long title truncates with an ellipsis, keeping the path.
        s.session_title = Some("x".repeat(120));
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf2 = terminal.backend().buffer();
        let long = (0u16..80)
            .map(|x| buf2[(x, 23)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>();
        assert!(
            long.contains('…'),
            "long title truncates with an ellipsis: {long:?}"
        );
        assert!(
            long.trim_end().ends_with(r"D:\MyProjects\Chore\dsh"),
            "path stays visible: {long:?}"
        );
        assert_eq!(buf2[(75, 23)].symbol(), "h", "path still flush right");
    }

    /// Baseline for the existing approval surface before it migrates to the
    /// shared input-accessory layout.
    #[test]
    fn approval_card_renders_above_the_input() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.approval = Some(crate::model::ApprovalCard {
            id: "approval-1".into(),
            tool_name: "bash".into(),
            reason: "需要执行外部命令".into(),
        });
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let backend = TestBackend::new(80, 14);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &Theme::ferra(),
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let all = (0u16..14)
            .map(|y| {
                (0u16..80)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .replace(' ', "");
        assert!(all.contains("审批·bash"), "approval heading: {all}");
        // The current fixed three-row panel has a one-row bordered body; this
        // baseline intentionally records that only the heading is visible.
        assert!(
            !all.contains("需要执行外部命令"),
            "reason is currently clipped: {all}"
        );
    }

    /// A pending question shrinks the transcript by a 3-row question panel,
    /// replaces the input bar with the selection bar (highlighted option
    /// reversed), and marks the status bar as waiting.
    #[test]
    fn question_bar_replaces_input_bar() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.msgs.push(Msg::User { text: "hi".into() });
        s.question = Some(crate::model::QuestionBatch::new(
            "r1".into(),
            "s1".into(),
            vec![crate::protocol::QuestionItem {
                id: "q1".into(),
                question: "选哪个?".into(),
                header: Some("选择".into()),
                options: Some(vec![
                    crate::protocol::QuestionOption {
                        label: "甲".into(),
                        description: Some("甲的说明".into()),
                    },
                    crate::protocol::QuestionOption {
                        label: "乙".into(),
                        description: None,
                    },
                ]),
                multi_select: false,
            }],
        ));
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0u16..80)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        // Layout: 12 = transcript(3: y0..3) + question panel(3: y3..6)
        // + selection bar block(3: y6..9, text row y7) + spacer(y9) +
        // status(y10) + title(y11). (CJK glyphs occupy two cells, so strip
        // spaces before matching.)
        let text = |y: u16| -> String { row(y).replace(' ', "") };
        assert!(text(3).contains("选择"), "panel title: {}", row(3));
        assert!(text(4).contains("选哪个?"), "question text: {}", row(4));
        assert!(
            text(5).contains("甲的说明"),
            "option description: {}",
            row(5)
        );
        let bar = row(7);
        assert!(
            bar.contains("◄") && bar.contains("►"),
            "selection bar: {bar}"
        );
        assert!(
            bar.contains('甲') && bar.contains('乙'),
            "options visible: {bar}"
        );
        // The highlighted option renders reversed (Night on Mist).
        let x = (0u16..80)
            .find(|&x| buf[(x, 7)].symbol() == "甲")
            .expect("甲 cell");
        let cell = &buf[(x, 7)];
        assert_eq!(cell.fg, Theme::ferra().bg, "selected option fg = Night");
        assert_eq!(cell.bg, Theme::ferra().fg, "selected option bg = Mist");
        // The right-side hint carries the confirm affordance.
        assert!(
            bar.replace(' ', "").contains("Enter确定"),
            "confirm hint: {bar}"
        );
        // Missing optional status values do not leave placeholder dashes.
        assert!(text(10).contains("•standard"), "status: {}", row(10));
        assert!(!text(10).contains('—') && !text(10).contains("CH"));
    }

    /// The pending-prompt queue renders above the input bar: one row per
    /// prompt, Night background with Bark text, `  * ` prefix, and long
    /// prompts truncate to a single row with `…`.
    #[test]
    fn prompt_queue_renders_above_input_bar() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.queue = vec!["这是马上要发的队列中的提示词".into(), "x".repeat(100)];
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0u16..80)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        // Layout: transcript(16) + queue(2, y16..18) + input(3, y18..21)
        // + gap(y21) + status(y22) + title(y23). (CJK glyphs occupy two
        // cells, so strip spaces before matching.)
        let compact = |y: u16| row(y).replace(' ', "");
        assert!(
            compact(16).starts_with("*这是马上要发的队列中的提示词"),
            "queue row 0: {}",
            row(16)
        );
        let q1 = row(17);
        assert!(compact(17).starts_with("*x"), "queue row 1: {q1}");
        assert!(q1.contains('…'), "long prompt truncates with …: {q1}");
        assert!(
            !compact(17).ends_with('x'),
            "truncated row stays on one line"
        );
        // Night background + Bark foreground on the queue text.
        let cell = &buf[(4, 16)];
        assert_eq!(cell.bg, theme.bg, "queue row bg = Night");
        assert_eq!(cell.fg, theme.dim, "queue row fg = Bark");
        // The queue must not spill into the input bar below.
        assert_eq!(
            buf[(4, 18)].bg,
            theme.bg_soft,
            "input bar untouched below the queue"
        );
        assert!(!row(18).contains('x'), "no overflow into the input bar");
    }

    #[test]
    fn todo_goal_and_plan_accessories_render_above_input() {
        use ratatui::backend::TestBackend;
        let mut config = crate::config::Config::default();
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.goal = Some("ship release".into());
        s.plan_mode = Some("on".into());
        s.todos = vec![("implement events".into(), "in-progress".into())];
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let backend = TestBackend::new(80, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &mut s,
                    &input,
                    &mut scroll,
                    &Theme::ferra(),
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let all = (0u16..20)
            .map(|y| {
                (0u16..80)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("Goal:") && all.contains("ship release"));
        assert!(all.contains("Plan:") && all.contains("on"));
        assert!(all.contains("Todo") && all.contains("implement events"));
    }

    /// The settings panel replaces the input bar (no floating window, no
    /// border/title), takes 2/3 of the page height on Ash, centers the
    /// category tabs, highlights only the focused NAME with Night, and
    /// renders choice options as `○ label` (default fg) / `● label` (green).
    #[test]
    fn settings_panel_replaces_input_and_focus_colors() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let mut settings = crate::settings::SettingsState::default(); // 外观 · 主题
                                                                      // The 主题 item lists discovered themes; give it two so the ○/● and
                                                                      // "custom" assertions below still have a non-selected option.
        settings.themes = vec!["ferra".into(), "custom".into()];
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let has_cell = |buf: &ratatui::buffer::Buffer, sym: char, fg: Color, bg: Color| -> bool {
            (0..24u16).any(|y| {
                (0..80u16).any(|x| {
                    let cell = &buf[(x, y)];
                    cell.symbol().chars().next() == Some(sym) && cell.fg == fg && cell.bg == bg
                })
            })
        };
        let text = |buf: &ratatui::buffer::Buffer| -> String {
            (0..24u16)
                .flat_map(|y| {
                    (0..80u16).map(move |x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                })
                .collect::<String>()
                .replace(' ', "")
        };
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: Some(&mut settings),
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let all = text(buf);
        // No floating window: no title.
        assert!(!all.contains("设置·/settings"), "no floating title");
        // 2/3 of the page height: settings occupies 16 rows; the status bar
        // sits above the bottom title row (y 22 of 24).
        let status_row: String = (0..80u16)
            .map(|x| buf[(x, 22)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            status_row.contains("• standard"),
            "status bar remains above the title row"
        );
        // Category tabs: centered and never highlighted (Ash background).
        let tab_cell = (0..24u16)
            .flat_map(|y| (0..80u16).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "外")
            .expect("tabs row rendered");
        assert!(
            (22..30).contains(&tab_cell.0),
            "tabs centered (外 at {}, page x 4..76)",
            tab_cell.0
        );
        assert_eq!(
            buf[tab_cell].bg, theme.bg_soft,
            "tabs are not selectable (Ash)"
        );
        // Focused name cell: Night background; descriptions stay Ash.
        assert!(
            has_cell(buf, '主', theme.fg, theme.bg),
            "focused name cell is Night"
        );
        assert!(
            has_cell(buf, 'C', theme.dim, theme.bg_soft),
            "description is Bark on Ash"
        );
        // Selected choice option: green ● on Ash.
        assert!(
            has_cell(buf, '●', theme.ok, theme.bg_soft),
            "selected option ● is green"
        );
        // Unselected choice option: ○ + default fg on Ash.
        assert!(
            has_cell(buf, '○', theme.fg, theme.bg_soft),
            "unselected option ○ is default fg"
        );
        // Unfocused value text stays on Ash.
        assert!(
            has_cell(buf, 'c', theme.fg, theme.bg_soft),
            "unfocused value stays Ash"
        );
        // Editing a choice: the cursor option is the focused (Night) element.
        settings.editing = Some(crate::settings::Edit::Choice { cursor: 1 });
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: Some(&mut settings),
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(
            has_cell(buf, 'c', theme.fg, theme.bg),
            "editing cursor option is Night"
        );
    }

    /// The /login panel replaces the input bar (same 2/3-height Ash shape as
    /// the settings panel). The three-way menu renders, Enter opens the
    /// provider sub-page (configured-key view with hint), and the API-key
    /// edit masks the typed secret.
    #[test]
    fn login_panel_replaces_input_bar() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let mut login = crate::login::LoginState::default();
        login.apply(crate::login::LoginView {
            providers: vec![crate::protocol::ProviderInfo {
                id: "deepseek".into(),
                name: "DeepSeek".into(),
                api_key_configured: true,
                api_key_writable: true,
                api_key_source: Some("file".into()),
                api_key_hint: Some("…1234".into()),
            }],
            proxies: vec![],
            error: None,
        });
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let all_text = |buf: &ratatui::buffer::Buffer| -> String {
            (0..24u16)
                .flat_map(|y| {
                    (0..80u16).map(move |x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                })
                .collect::<String>()
                .replace(' ', "")
        };
        // Menu page: the two choices render.
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: Some(&mut login),
                    },
                )
            })
            .unwrap();
        let all = all_text(terminal.backend().buffer());
        assert!(all.contains("登录"), "login title");
        assert!(
            all.contains("APIkey"),
            "API key menu item (spaces stripped)"
        );
        assert!(all.contains("Proxy"), "Proxy menu item");
        // Provider sub-page: the provider row shows the configured-key view.
        login.page = crate::login::Page::Providers;
        login.loading = false;
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: Some(&mut login),
                    },
                )
            })
            .unwrap();
        let all = all_text(terminal.backend().buffer());
        assert!(all.contains("DeepSeek"), "provider row: {all:?}");
        assert!(all.contains("已配置…1234"), "configured key hint");
        assert!(!all.contains("sk-"), "no secret on screen");
        // API-key edit masks the typed secret as bullets.
        login.page = crate::login::Page::ApiKey {
            provider: "deepseek".into(),
            buf: String::new(),
        };
        login.editing = Some("sk-secret".into());
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: Some(&mut login),
                    },
                )
            })
            .unwrap();
        let all = all_text(terminal.backend().buffer());
        assert!(all.contains("●●●●●●●●●█"), "typed key renders as bullets");
        assert!(
            !all.contains("sk-secret"),
            "typed secret never renders in plain text"
        );
    }

    #[test]
    fn large_plugin_command_directory_scrolls_inside_a_bounded_popup() {
        use ratatui::backend::TestBackend;

        let suggest = Suggestion {
            query: "/".into(),
            sel: 29,
            matches: (0..30).map(|i| format!("/plugin-{i:02}")).collect(),
            descriptions: (0..30).map(|_| "plugin command".into()).collect(),
            sources: (0..30).map(|_| CommandSource::Integrated).collect(),
            kind: SuggestionKind::Commands,
        };
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_suggest(
                    frame,
                    &suggest,
                    ratatui::layout::Rect::new(4, 34, 72, 3),
                    &theme,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let all: String = (0..40u16)
            .flat_map(|y| (0..80u16).map(move |x| buf[(x, y)].symbol().to_owned()))
            .collect();
        assert!(all.contains("/plugin-29"), "selected tail remains visible");
        assert!(
            !all.contains("/plugin-00"),
            "popup renders a bounded window"
        );
    }

    #[test]
    fn skill_suggestion_popup_has_skill_header_and_canonical_lines() {
        use ratatui::backend::TestBackend;

        let suggest = Suggestion {
            query: "/skill".into(),
            sel: 0,
            matches: vec!["/skill:code-review".into()],
            descriptions: vec!["Review a change".into()],
            sources: vec![CommandSource::Builtin],
            kind: SuggestionKind::Skills,
        };
        let backend = TestBackend::new(80, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_suggest(
                    frame,
                    &suggest,
                    ratatui::layout::Rect::new(4, 16, 72, 3),
                    &Theme::ferra(),
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let all: String = (0..20u16)
            .flat_map(|y| (0..80u16).map(move |x| buf[(x, y)].symbol().to_owned()))
            .collect();
        let compact = all.replace(' ', "");
        assert!(compact.contains("技能"));
        assert!(compact.contains("/skill:code-review"));
        assert!(compact.contains("Reviewachange"));
    }

    /// Regression: the suggestion popup must be fully opaque — no transcript
    /// text may show through its background.
    #[test]
    fn suggest_popup_is_opaque_over_transcript() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        for i in 0..30 {
            s.msgs.push(Msg::User {
                text: format!("第{i}行 用户消息内容 漏漏漏漏漏漏"),
            });
        }
        let mut input = InputState::new(&config);
        input.handle_key(&KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), true);
        assert!(input.suggest.is_some());
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // The popup height follows the centralized command catalog; adding a
        // discovered/built-in command must not make this opacity test stale.
        let popup_height = (input.suggest.as_ref().unwrap().matches.len().min(12) + 2) as u16;
        let input_y = 34; // 40 - input(3) - gap/status/title(3)
        let rect = ratatui::layout::Rect::new(4, input_y - popup_height, 46, popup_height);
        for y in rect.y..rect.y + rect.height {
            let row: String = (rect.x..rect.x + rect.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect();
            // The transcript behind uses these CJK characters; any of them
            // inside the popup means text bled through.
            assert!(
                !row.contains('漏'),
                "transcript text bleeds into popup row {y}: {row}"
            );
        }
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                // Continuation cells of wide chars are skipped by the
                // terminal diff (the wide glyph covers them) — not bleed.
                let continuation = x > rect.x && buf[(x - 1, y)].symbol().width() > 1;
                if continuation {
                    continue;
                }
                let cell = &buf[(x, y)];
                assert_ne!(
                    cell.bg,
                    ratatui::style::Color::Reset,
                    "popup cell ({x},{y}) transparent, symbol={:?}",
                    cell.symbol()
                );
            }
        }
    }

    /// Regression: rendering the input bar with a CJK buffer used to panic
    /// ("byte index is not a char boundary") because the char cursor index
    /// was used to slice the buffer by byte. It must render and place the
    /// terminal cursor at the right display column (CJK = 2 cells).
    #[test]
    fn input_bar_renders_cjk_without_panic() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut input = InputState::new(&config);
        for c in "你好世界".chars() {
            input.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), true);
        }
        input.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), true);

        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = Theme::ferra();
        let mut cursor_anchor = None;
        terminal
            .draw(|frame| {
                cursor_anchor = render_input(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 80, 5),
                    &input,
                    &theme,
                    false,
                    None,
                    2,
                );
            })
            .unwrap();
        // Cursor sits after "你好世" (6 cells) inside the 2-column gutter:
        // x = 2 + 6 = 8; one padding row above the input line means y = 1.
        assert_eq!(
            cursor_anchor,
            Some(Position::new(8, 1)),
            "hidden IME anchor tracks CJK display width"
        );
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(0, 0),
            "renderer does not expose or move the hardware cursor"
        );
    }

    /// Regression: Shift+Enter before typing any text must still grow the
    /// input bar by one row — a trailing newline is a second display line,
    /// which `str::lines()` undercounts.
    #[test]
    fn shift_enter_on_empty_buffer_grows_input_bar() {
        use crate::input::InputAction;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut input = InputState::new(&config);
        let action = input.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
        assert_eq!(action, InputAction::None, "Shift+Enter must not send");
        assert_eq!(input.buf, "\n");
        assert!(input.multiline);
        assert_eq!(input_rows(&input), 2, "trailing newline is a second row");

        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = Theme::ferra();
        let mut cursor_anchor = None;
        terminal
            .draw(|frame| {
                cursor_anchor = render_input(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 80, 6),
                    &input,
                    &theme,
                    false,
                    None,
                    2,
                );
            })
            .unwrap();
        // Cursor on the new empty row: 1 padding row + row 1 → y = 2.
        assert_eq!(
            cursor_anchor,
            Some(Position::new(2, 2)),
            "hidden IME anchor sits on the new empty row"
        );
    }

    /// Long content wraps inside the input bar instead of overflowing, and
    /// the cursor follows the wrapped rows.
    #[test]
    fn input_bar_wraps_long_content() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut input = InputState::new(&config);
        input.buf = "x".repeat(60);
        input.cursor = input.buf.chars().count();
        let theme = Theme::ferra();
        let backend = TestBackend::new(20, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut cursor_anchor = None;
        terminal
            .draw(|f| {
                cursor_anchor = render_input(
                    f,
                    ratatui::layout::Rect::new(0, 0, 20, 6),
                    &input,
                    &theme,
                    false,
                    None,
                    0,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0u16..20)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        // 60 columns at width 20 → three wrapped rows inside the box.
        assert_eq!(row(1), "x".repeat(20), "wrapped row 1");
        assert_eq!(row(2), "x".repeat(20), "wrapped row 2");
        assert_eq!(row(3), "x".repeat(20), "wrapped row 3");
        assert_eq!(
            cursor_anchor,
            Some(Position::new(20, 3)),
            "hidden IME anchor follows the wrapped rows to the end"
        );
    }

    /// Long user-message lines wrap inside the transcript with the soft
    /// background filling every wrapped row (no ragged block edges).
    #[test]
    fn user_block_long_lines_stay_solid() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.msgs.push(Msg::User {
            text: "x".repeat(150),
        });
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Page x = (80 - 72) / 2 = 4; 150 columns wrap into 3 rows (72/72/6)
        // between the block's two padding rows (y0, y4).
        for y in 1..=3u16 {
            for x in 4u16..76 {
                assert_eq!(
                    buf[(x, y)].bg,
                    theme.bg_soft,
                    "solid user block at ({x},{y})"
                );
            }
        }
    }

    /// Perf regression: a streaming chunk must splice only the cached tail,
    /// leaving earlier messages untouched and the line count consistent.
    #[test]
    fn tail_splice_updates_only_the_streaming_message() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.apply_event(&serde_json::json!({
            "type": "user/message", "seq": 1, "time": 0,
            "data": {
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }
        }));
        s.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 2, "time": 0,
            "data": {"chunk": {"type": "text-delta", "text": "流"}}
        }));
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let area = ratatui::layout::Rect::new(0, 0, 80, 20);
        // Frame 1: full rebuild.
        terminal
            .draw(|f| render_transcript(f, area, &mut s, &mut scroll, &theme, false, None))
            .unwrap();
        let len1 = s.transcript_cache.lines.len();
        // Append a chunk: cache stays valid, only the tail is dirty.
        s.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 3, "time": 0,
            "data": {"chunk": {"type": "text-delta", "text": "式"}}
        }));
        assert!(s.transcript_cache.valid && s.transcript_cache.tail_dirty);
        terminal
            .draw(|f| render_transcript(f, area, &mut s, &mut scroll, &theme, false, None))
            .unwrap();
        // Streaming tail replaced (1 line + gap): same total length.
        assert_eq!(
            s.transcript_cache.lines.len(),
            len1,
            "tail splice keeps the line count"
        );
        let joined: String = s
            .transcript_cache
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|sp| sp.content.as_ref()))
            .collect();
        assert!(
            joined.contains("流式"),
            "spliced tail shows the full stream"
        );
        assert!(
            !s.transcript_cache.tail_dirty,
            "tail dirty flag cleared after render"
        );
    }

    /// History prepend: the viewport must stay anchored on the previously
    /// visible content — the scroll offset shifts by the lines added above.
    #[test]
    fn history_prepend_keeps_viewport() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.apply_event(&serde_json::json!({
            "type": "user/message", "seq": 10, "time": 0,
            "data": {
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }
        }));
        s.history_exhausted = false;
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let area = ratatui::layout::Rect::new(0, 0, 80, 20);
        terminal
            .draw(|f| render_transcript(f, area, &mut s, &mut scroll, &theme, false, None))
            .unwrap();
        let old_len = s.transcript_cache.lines.len();
        // Load an older page while the user is at the top.
        scroll.follow = false;
        scroll.offset = 0;
        s.prepend_events(&[serde_json::json!({
            "type": "user/message", "seq": 2, "time": 0,
            "data": {
                "content": [{"type": "text", "text": "前"}],
                "source": {"kind": "user"}
            }
        })]);
        terminal
            .draw(|f| render_transcript(f, area, &mut s, &mut scroll, &theme, false, None))
            .unwrap();
        let delta = s.transcript_cache.lines.len() - old_len;
        assert_eq!(scroll.offset, delta, "viewport shifted by the added lines");
        assert!(delta > 0);
        // The previously visible message is still what's on screen.
        let buf = terminal.backend().buffer();
        let mut found_hi = false;
        for y in 0..20 {
            let row: String = (0..80)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect();
            if row.contains("hi") {
                found_hi = true;
            }
            assert!(
                !row.contains("前"),
                "older history is above the viewport: {row}"
            );
        }
        assert!(found_hi, "previous content still visible");
    }

    /// The configured page max width caps the content area (centered) and
    /// long transcript lines wrap at that width.
    #[test]
    fn page_max_width_caps_and_wraps() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.config.page_max_width = 40;
        let long = "x".repeat(80);
        s.msgs.push(Msg::Assistant {
            text: long.clone(),
            lines: vec![RenderLine {
                line: Line::from(long.clone()),
                unit: 0,
                raw_line: Some(0),
                atomic: false,
                fill: false,
            }],
            unit_start: 0,
        });
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Content width capped at 40, centered on 80 → x = 20..60.
        let row0: String = (20..60)
            .map(|x| buf[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        let row1: String = (20..60)
            .map(|x| buf[(x, 1)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert_eq!(row0, "x".repeat(40));
        assert_eq!(row1, "x".repeat(40), "long line wraps at the page width");
        assert_eq!(
            buf[(19, 0)].bg,
            ratatui::style::Color::Reset,
            "left of the page is empty"
        );
        assert_eq!(
            buf[(60, 0)].bg,
            ratatui::style::Color::Reset,
            "right of the page is empty"
        );
    }

    /// Activity rows use the configured page width for their ellipsis. A
    /// narrower page must not wrap a tool summary that still fits the terminal.
    #[test]
    fn tool_row_truncates_at_page_width_without_wrapping() {
        use crate::model::{ToolCard, ToolState};
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        config.resolved_theme = Theme::ferra();
        config.page_max_width = 40;
        let mut s = AppState::default();
        s.config = config.clone();
        s.msgs.push(Msg::Tool(ToolCard {
            call_id: "wide-tool".into(),
            name: "bash".into(),
            summary: "x".repeat(70),
            state: ToolState::Running,
            frame: 0,
            start_ms: 0,
            done_since: None,
            done_from: None,
        }));
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let tool_row: String = (20..60)
            .map(|x| buf[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        let next_row: String = (20..60)
            .map(|x| buf[(x, 1)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert_eq!(tool_row.width(), 40);
        assert!(
            tool_row.ends_with('…'),
            "ellipsis uses the 40-column page edge: {tool_row:?}"
        );
        assert!(
            next_row.trim().is_empty(),
            "tool activity remains a single display row: {next_row:?}"
        );
        assert_eq!(s.transcript_cache.display_len(), 2, "row plus message gap");
    }

    /// The Thinking row renders like a tool card: colored (breathing) bullet
    /// while running, green once the phase completes.
    #[test]
    fn thinking_renders_like_tool_card() {
        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.start_thinking();
        let theme = Theme::ferra();
        let lines = legacy_test_lines(s.msgs.last().unwrap(), &s);
        let text: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert_eq!(text, "  • Thinking...", "tool-card shape");
        let bullet = &lines[0].spans[1];
        assert_eq!(bullet.content, "•");
        assert!(
            bullet.style.fg.is_some(),
            "running bullet is colored (breathing)"
        );
        // Settle: the bullet ends green.
        s.stop_thinking();
        if let Some(Msg::Thinking(card)) = s.msgs.last_mut() {
            card.done_since = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
            card.count = 5;
        }
        let lines = legacy_test_lines(s.msgs.last().unwrap(), &s);
        assert_eq!(
            lines[0].spans[1].style.fg,
            Some(theme.ok),
            "done bullet settles green"
        );
        let text: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert_eq!(
            text, "  • Thinking... x5",
            "consecutive phases show a count"
        );
    }

    /// Code/mermaid fill rows use the Night background (#2b292d, bg) and
    /// pad to the full area width.
    #[test]
    fn code_block_fill_uses_night() {
        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config;
        s.msgs.push(Msg::Assistant {
            text: "```\ncode\n```".into(),
            lines: vec![
                RenderLine {
                    line: Line::from(Span::styled(
                        "  code · 1 行",
                        Style::default().fg(Theme::ferra().dim),
                    )),
                    unit: 0,
                    raw_line: Some(0),
                    atomic: true,
                    fill: true,
                },
                RenderLine {
                    line: Line::from(Span::styled(
                        "  code",
                        Style::default().fg(Theme::ferra().fg),
                    )),
                    unit: 0,
                    raw_line: Some(1),
                    atomic: true,
                    fill: true,
                },
            ],
            unit_start: 0,
        });
        let lines = legacy_test_styled_lines(&s.msgs[0], &s, 80);
        let theme = Theme::ferra();
        for line in &lines {
            assert_eq!(line.width(), 80, "fill rows pad to the area width");
            // The line base style carries the Night background (#2b292d, the
            // bg slot; spans inherit it at render time).
            assert_eq!(
                line.style.bg,
                Some(theme.bg),
                "fill rows use the Night background"
            );
            if let Some(last) = line.spans.last() {
                assert_eq!(last.style.bg, Some(theme.bg), "pad span carries the bg");
            }
        }
    }

    #[test]
    fn inline_code_background_does_not_fill_the_rest_of_the_row() {
        use ratatui::backend::TestBackend;

        let mut theme = Theme::ferra();
        let chip_bg = Color::Rgb(1, 2, 3);
        theme.markdown.inline_code.bg = Some(chip_bg);
        let mut next_unit = 0;
        let mut units = std::collections::HashMap::new();
        let lines = crate::render::render_markdown(
            "foo `hello` bar",
            &theme,
            &mut next_unit,
            &crate::render::RenderOptions::default(),
            &mut units,
        );
        let mut config = crate::config::Config::default();
        config.resolved_theme = theme;
        let mut state = AppState::default();
        state.config = config.clone();
        state.msgs.push(Msg::Assistant {
            text: "foo `hello` bar".into(),
            lines,
            unit_start: 0,
        });
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let backend = TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_with_cursor(
                    frame,
                    &mut state,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let (start_x, y) = (0..12)
            .find_map(|y| {
                let row: String = (0..40).map(|x| buffer[(x, y)].symbol()).collect();
                row.find("foo").map(|x| (x as u16, y))
            })
            .expect("markdown row is visible");
        assert_eq!(buffer[(start_x + 4, y)].bg, chip_bg, "chip leading pad");
        assert_eq!(buffer[(start_x + 10, y)].bg, chip_bg, "chip trailing pad");
        let row_bg = buffer[(start_x + 11, y)].bg;
        assert_ne!(
            row_bg, chip_bg,
            "source separator after the chip must not inherit chip bg"
        );
        assert_eq!(
            buffer[(start_x + 15, y)].bg,
            row_bg,
            "cells after the paragraph keep the ordinary row background"
        );
    }

    /// Consecutive activity rows (tool cards + read/edit groups) render
    /// glued together — no gap row between them.
    #[test]
    fn tool_and_file_group_are_glued_without_gap() {
        use crate::model::{FileAction, FileGroup, FileItem, ToolCard, ToolState};
        use ratatui::backend::TestBackend;

        let tool = || {
            Msg::Tool(ToolCard {
                call_id: "b1".into(),
                name: "bash".into(),
                summary: "cmd".into(),
                state: ToolState::Running,
                frame: 0,
                start_ms: 0,
                done_since: None,
                done_from: None,
            })
        };
        let group = || {
            Msg::FileGroup(FileGroup {
                items: vec![FileItem {
                    action: FileAction::Edit,
                    call_id: "e1".into(),
                    file: "a.rs".into(),
                    ok: None,
                }],
                frame: 0,
                done_since: None,
                done_from: None,
            })
        };

        let mut s = AppState::default();
        s.config = crate::config::Config::default();
        s.msgs.push(tool());
        s.msgs.push(group());
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let area = ratatui::layout::Rect::new(0, 0, 80, 20);
        terminal
            .draw(|f| render_transcript(f, area, &mut s, &mut scroll, &theme, false, None))
            .unwrap();
        // tool(1) + glued group line(1) + final gap(1) = 3 rows.
        assert_eq!(s.transcript_cache.lines.len(), 3);
        assert!(
            s.transcript_cache.lines[1].width() > 0,
            "no gap row between tool and file group"
        );
        assert_eq!(
            s.transcript_cache.lines[2].width(),
            0,
            "gap before the input bar"
        );

        // A hidden reasoning block is transparent: it must not split two
        // visible activity rows in either render or copy provenance layout.
        let mut hidden = AppState::default();
        hidden.config = crate::config::Config::default();
        hidden.msgs.push(tool());
        hidden.msgs.push(Msg::Block(TranscriptBlock {
            id: DisplayId::event(90, "reasoning"),
            unit: None,
            content: "hidden".into(),
            format: TranscriptFormat::Reasoning,
            tone: DisplayTone::Dim,
            copy_source: "hidden".into(),
            streaming: false,
        }));
        hidden.msgs.push(group());
        hidden.msgs.push(Msg::Card(ContentCard {
            id: DisplayId::event(91, "context"),
            unit: Some(91),
            header: None,
            content: "visible card".into(),
            role: CardRole::Detail,
            tone: DisplayTone::Normal,
            horizontal_padding: 0,
            copy_source: "visible card".into(),
        }));
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut hidden_scroll = ScrollState::default();
        terminal
            .draw(|f| {
                render_transcript(
                    f,
                    area,
                    &mut hidden,
                    &mut hidden_scroll,
                    &theme,
                    false,
                    None,
                )
            })
            .unwrap();
        assert!(
            hidden.transcript_cache.lines[1].width() > 0,
            "hidden reasoning does not insert a gap between activities"
        );
        let card_start = copy_layout_rows(&hidden)
            .into_iter()
            .find(|row| row.unit == 91)
            .unwrap()
            .global_row;
        assert_eq!(
            card_start, 3,
            "copy layout uses the same visible activity adjacency"
        );

        // A user message above the tool keeps its gap row.
        let mut s2 = AppState::default();
        s2.config = crate::config::Config::default();
        s2.msgs.push(Msg::User { text: "hi".into() });
        s2.msgs.push(tool());
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut scroll = ScrollState::default();
        terminal
            .draw(|f| render_transcript(f, area, &mut s2, &mut scroll, &theme, false, None))
            .unwrap();
        // user(3) + gap + tool(1) + final gap = 6 rows.
        assert_eq!(s2.transcript_cache.lines.len(), 6);
        assert_eq!(
            s2.transcript_cache.lines[3].width(),
            0,
            "gap kept between user and tool"
        );
        assert!(s2.transcript_cache.lines[4].width() > 0);
    }

    /// Wrap-line splits long lines at the page width; exact-width fill rows
    /// pass through untouched (no phantom empty row).
    #[test]
    fn wrap_line_splits_long_lines_and_keeps_fill_intact() {
        let long: Line<'static> = Line::from("x".repeat(80));
        let out = wrap_line(long, 40);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].width(), 40);
        assert_eq!(out[1].width(), 40);
        // CJK glyphs are 2 columns: 4 chars (8 cells) at width 6 → 3 + 1.
        let cjk: Line<'static> = Line::from("你好世界");
        let out = wrap_line(cjk, 6);
        assert_eq!(out.len(), 2, "got {} rows", out.len());
        assert_eq!(out[0].width(), 6);
        assert_eq!(out[1].width(), 2);
        // An exact-width fill row must not split (the WordWrapper phantom
        // row came from exactly this case).
        let fill: Line<'static> = Line::from(vec![Span::styled(
            " ".repeat(40),
            Style::default().bg(Theme::ferra().bg_soft),
        )]);
        let out = wrap_line(fill.clone(), 40);
        assert_eq!(out.len(), 1, "exact-width rows must not split");
        assert_eq!(out[0].width(), 40);
    }

    #[test]
    fn wrap_line_keeps_combining_and_zwj_graphemes_intact() {
        let combining = Line::from(vec![
            Span::styled("a", Style::default().fg(Theme::ferra().fg)),
            Span::styled("\u{301}b", Style::default().fg(Theme::ferra().dim)),
        ]);
        let rows = wrap_line(combining, 1);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>(),
            "a\u{301}"
        );
        assert_eq!(
            rows[1]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>(),
            "b"
        );

        let rows = wrap_line(Line::from("👩‍💻x"), 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>(),
            "👩‍💻"
        );
        assert_eq!(
            rows[1]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>(),
            "x"
        );
    }

    /// The wrapped-row counter must agree with the splitter for every case
    /// the viewport math relies on (CJK straddles, exact width, multi-span).
    #[test]
    fn wrapped_rows_matches_wrap_line() {
        let cases: Vec<(Line<'static>, usize)> = vec![
            (Line::default(), 40),
            (Line::from("x".repeat(40)), 40),
            (Line::from("x".repeat(41)), 40),
            (Line::from("x".repeat(80)), 40),
            (Line::from("你".repeat(20)), 40),
            (Line::from("你好世界"), 6),
            // CJK straddle right after a short row: the splitter flushes
            // early and can emit one row more than ceil(cols/width).
            (Line::from("aaaaa你你你你你你a"), 6),
            (Line::from("aaa你"), 4),
            (
                Line::from(vec![
                    Span::styled("aaaaa", Style::default().fg(Theme::ferra().fg)),
                    Span::styled("你你你你你你a", Style::default().fg(Theme::ferra().dim)),
                ]),
                6,
            ),
        ];
        for (line, width) in cases {
            let split = wrap_line(line.clone(), width).len();
            let count = wrapped_rows(&line, width);
            assert_eq!(
                split, count,
                "row count mismatch at width {width}: {line:?}"
            );
        }
    }

    /// Regression: with a full viewport, the wrapped tail of the last
    /// message and the trailing gap row (the space before the input bar)
    /// must stay on screen — the old bottom-truncation pushed both off.
    #[test]
    fn follow_pins_wrapped_tail_and_gap_row() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.apply_event(&serde_json::json!({
            "type": "user/message", "seq": 1, "time": 0,
            "data": {
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "user"}
            }
        }));
        let long = "x".repeat(120);
        s.apply_event(&serde_json::json!({
            "type": "assistant/message", "seq": 2, "time": 1,
            "data": {"message": {"content": [{"type": "text", "text": long}]}}
        }));
        let mut scroll = ScrollState::default();
        scroll.follow = true;
        let theme = Theme::ferra();
        let backend = TestBackend::new(40, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 40, 4);
        terminal
            .draw(|f| render_transcript(f, area, &mut s, &mut scroll, &theme, false, None))
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0u16..40)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        // The 120-column line wraps to 3 rows at width 40, all visible…
        assert_eq!(row(0), "x".repeat(40), "wrapped tail row 1");
        assert_eq!(row(1), "x".repeat(40), "wrapped tail row 2");
        assert_eq!(row(2), "x".repeat(40), "wrapped tail row 3");
        // …and the trailing gap row stays as the last row before the input
        // bar instead of being truncated away.
        assert_eq!(row(3), " ".repeat(40), "gap row before the input bar");
    }

    /// Regression: user blocks must render as solid background rows — no
    /// background-less gap rows inside the block.
    #[test]
    fn user_block_has_no_bg_gaps() {
        use ratatui::backend::TestBackend;

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config.clone();
        s.msgs.push(Msg::User {
            text: "你好世界".into(),
        });
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    &mut s,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        input_page: None,
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Block = padding + content + padding rows (y 0..3), all solid.
        // (Skip wide-glyph continuation cells — the terminal diff leaves
        // them blank by design.)
        for y in 0..3 {
            for x in 4..76 {
                let continuation = x > 4 && buf[(x - 1, y)].symbol().width() > 1;
                if continuation {
                    continue;
                }
                assert_eq!(
                    buf[(x, y)].bg,
                    theme.bg_soft,
                    "no gap inside the user block at ({x},{y})"
                );
            }
        }
        // The message gap below is plain transcript background.
        assert_eq!(buf[(4, 3)].bg, ratatui::style::Color::Reset);
    }

    /// Failed read/edit items are listed on their own red lines and never
    /// join the folded green line.
    #[test]
    fn failed_file_items_are_listed_separately() {
        use crate::model::{FileAction, FileGroup, FileItem};

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config;
        s.msgs.push(Msg::FileGroup(FileGroup {
            items: vec![
                FileItem {
                    action: FileAction::Read,
                    call_id: "r1".into(),
                    file: "src/ok.rs".into(),
                    ok: Some(true),
                },
                FileItem {
                    action: FileAction::View,
                    call_id: "r2".into(),
                    file: "src/bad.rs".into(),
                    ok: Some(false),
                },
                FileItem {
                    action: FileAction::Replace,
                    call_id: "e1".into(),
                    file: "src/fix.rs".into(),
                    ok: Some(false),
                },
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        }));
        let lines = legacy_test_lines(&s.msgs[0], &s);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|sp| sp.content.as_ref()).collect())
            .collect();
        // Folded line contains only the successful read.
        assert_eq!(
            text[0], "  • read ok.rs",
            "failed items stay out of the fold"
        );
        assert!(
            text.iter().any(|l| l.contains("bad.rs")),
            "failed read listed: {text:?}"
        );
        assert!(
            text.iter().any(|l| l.contains("fix.rs")),
            "failed edit listed: {text:?}"
        );
        assert_eq!(lines.len(), 3, "fold + one line per failed item");
        let theme = Theme::ferra();
        let merged_bullet = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "•")
            .expect("merged bullet");
        assert_eq!(
            merged_bullet.style.fg,
            Some(theme.ok),
            "merged bullet green"
        );
        for line in &lines[1..] {
            let b = line
                .spans
                .iter()
                .find(|sp| sp.content == "•")
                .expect("fail bullet");
            assert_eq!(b.style.fg, Some(theme.err), "failed bullets red");
        }
    }

    /// Repeated reads/edits of the same file collapse into `name xN` on the
    /// group lines; failed repeats stay one line per distinct file (matching
    /// `file_group_line_count`).
    #[test]
    fn transcript_height_and_wheel_step_follow_the_visible_layout() {
        let config = crate::config::Config::default();
        let state = AppState::default();
        let input = InputState::new(&config);
        assert_eq!(transcript_view_height(40, &state, &input, false), 34);
        assert_eq!(transcript_view_height(40, &state, &input, true), 11);

        let mut scroll = ScrollState {
            offset: 50,
            follow: false,
        };
        scroll_lines(&mut scroll, 11, 100, true, 3);
        assert_eq!(scroll.offset, 47, "one wheel tick moves three rows");
        scroll_page(&mut scroll, 11, 100, true);
        assert_eq!(scroll.offset, 37, "PageUp moves one visible page");
    }

    #[test]
    fn input_page_shell_has_exact_padding_and_small_layout_is_bounded() {
        use ratatui::backend::TestBackend;

        let theme = Theme::ferra();
        let config = crate::config::Config::default();
        let mut page = crate::input_page::InputPageSession::model();
        let backend = TestBackend::new(20, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_input_page(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 20, 8),
                    &mut page,
                    &config,
                    &theme,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0..20u16)
                .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        assert!(row(0).trim().is_empty(), "one blank top padding row");
        assert!(row(7).trim().is_empty(), "one blank bottom padding row");
        assert_eq!(buffer[(0, 1)].symbol(), " ");
        assert_eq!(buffer[(1, 1)].symbol(), " ");
        assert_eq!(
            buffer[(2, 1)].symbol(),
            "❯",
            "content starts after two columns"
        );
    }

    #[test]
    fn all_input_pages_are_bounded_on_a_short_terminal() {
        use ratatui::backend::TestBackend;

        let theme = Theme::ferra();
        let config = crate::config::Config::default();
        let files = vec![crate::theme::ThemeFile::from_theme("ferra", theme)];
        let mut pages = vec![
            crate::input_page::InputPageSession::settings(crate::settings::SettingsState::default()),
            crate::input_page::InputPageSession::login(),
            crate::input_page::InputPageSession::model(),
            crate::input_page::InputPageSession::theme(&files, "ferra"),
            crate::input_page::InputPageSession::resume(),
        ];
        for page in &mut pages {
            let backend = TestBackend::new(14, 5);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| {
                    render_input_page(
                        frame,
                        ratatui::layout::Rect::new(0, 0, 14, 5),
                        page,
                        &config,
                        &theme,
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert!((0..14u16).all(|x| buffer[(x, 0)].symbol() == " "));
            assert!((0..5u16)
                .all(|y| { buffer[(0, y)].symbol() == " " && buffer[(1, y)].symbol() == " " }));
        }
    }

    #[test]
    fn input_page_replaces_editor_without_touching_status_title_or_draft() {
        use ratatui::backend::TestBackend;

        let theme = Theme::ferra();
        let config = crate::config::Config::default();
        let mut state = AppState::default();
        state.config = config.clone();
        let mut input = InputState::new(&config);
        input.buf = "keep this draft".into();
        input.cursor = input.buf.chars().count();
        let mut page = crate::input_page::InputPageSession::model();
        let mut scroll = ScrollState::default();
        let backend = TestBackend::new(60, 18);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &mut state,
                    &input,
                    &mut scroll,
                    &theme,
                    RenderOverlays {
                        help_visible: false,
                        overlay: None,
                        toast: None,
                        input_page: Some(&mut page),
                        settings: None,
                        login: None,
                    },
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0..60u16)
                .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        assert!(row(16).contains("^h Help"));
        assert!(row(17).replace(' ', "").contains("新会话"));
        assert_eq!(input.buf, "keep this draft");
    }

    #[test]
    fn resume_input_page_renders_titles_without_an_overlay() {
        use ratatui::backend::TestBackend;

        let theme = Theme::ferra();
        let config = crate::config::Config::default();
        let mut page = crate::input_page::InputPageSession::resume();
        page.apply_sessions(
            vec![crate::protocol::SessionInfo {
                id: "session-1".into(),
                title: "actual session title".into(),
                live: true,
                created_at: 1,
            }],
            false,
        );
        let backend = TestBackend::new(60, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_input_page(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 60, 12),
                    &mut page,
                    &config,
                    &theme,
                )
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.symbol() == "续"));
        assert!(text.contains("actual session title"));
        assert!(!text.contains('┌') && !text.contains('┐'));
    }

    #[test]
    fn theme_input_page_renders_without_overlay_border() {
        use ratatui::backend::TestBackend;

        let theme = Theme::ferra();
        let config = crate::config::Config::default();
        let files = vec![crate::theme::ThemeFile::from_theme("ferra", theme)];
        let mut page = crate::input_page::InputPageSession::theme(&files, "ferra");
        let backend = TestBackend::new(50, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_input_page(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 50, 12),
                    &mut page,
                    &config,
                    &theme,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..12u16 {
            for x in 0..50u16 {
                text.push_str(buffer[(x, y)].symbol());
            }
        }
        assert!(
            !text.contains('┌') && !text.contains('┐'),
            "Input Page has no border"
        );
    }

    #[test]
    fn file_group_collapses_operations_and_repeats_into_counts() {
        use crate::model::{FileAction, FileGroup, FileItem};

        let item = |action: FileAction, id: &str, file: &str, ok: bool| FileItem {
            action,
            call_id: id.into(),
            file: file.into(),
            ok: Some(ok),
        };
        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config;
        s.msgs.push(Msg::FileGroup(FileGroup {
            items: vec![
                item(FileAction::Read, "r1", "src/foo.rs", true),
                item(FileAction::Read, "r2", "src/foo.rs", true),
                item(FileAction::View, "v1", "src/view.rs", true),
                item(FileAction::Edit, "e1", "src/model.rs", true),
                item(FileAction::Edit, "e2", "src/model.rs", true),
                item(FileAction::Replace, "p1", "src/bar.rs", true),
                item(FileAction::Insert, "i1", "world.rs", true),
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        }));
        let lines = legacy_test_lines(&s.msgs[0], &s);
        let text: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert_eq!(
            text,
            "  • read foo.rs x2; view view.rs; edit model.rs x2; replace bar.rs; insert world.rs",
            "operation labels survive folding and repeats fold into xN"
        );
        // Failed repeats: one red line per DISTINCT action/file, with count.
        let group = FileGroup {
            items: vec![
                item(FileAction::View, "v1", "src/bad.rs", false),
                item(FileAction::View, "v2", "src/bad.rs", false),
                item(FileAction::View, "v3", "src/bad.rs", false),
            ],
            frame: 0,
            done_since: None,
            done_from: None,
        };
        s.msgs[0] = Msg::FileGroup(group.clone());
        let lines = legacy_test_lines(&s.msgs[0], &s);
        assert_eq!(lines.len(), 1, "three failures of one file = one line");
        let text: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert_eq!(text, "  • view bad.rs x3");
        assert_eq!(
            crate::model::file_group_line_count(&group),
            1,
            "copy-mode row math matches the collapsed line"
        );
    }

    #[test]
    fn animation_patches_only_the_active_message_in_a_long_transcript() {
        use ratatui::backend::TestBackend;

        let mut state = AppState::default();
        for index in 0..500 {
            state.push_system_message(format!("settled {index}"));
        }
        state.start_thinking();
        let active_index = state.transcript.len() - 1;
        state.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 9_999,
            "data": {"chunk": {"type": "reasoning-delta", "text": "hidden stream"}}
        }));
        let mut scroll = ScrollState::default();
        let theme = state.theme();
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 80, 20);
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        state.transcript_cache.take_work_stats();
        let settled_prefix = state.transcript_cache.lines[..20].to_vec();

        assert!(crate::model::tick_spinners(
            &mut state,
            std::time::Instant::now()
        ));
        assert!(state.transcript_cache.valid);
        assert_eq!(
            state
                .transcript_cache
                .dirty_messages
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![active_index]
        );
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        let work = state.transcript_cache.take_work_stats();
        assert_eq!(work.rebuilds, 0);
        assert_eq!(work.patches, 1);
        assert_eq!(
            &state.transcript_cache.lines[..20],
            settled_prefix.as_slice()
        );
    }

    #[test]
    fn dirty_message_line_count_mismatch_falls_back_to_rebuild() {
        use ratatui::backend::TestBackend;

        let mut state = AppState::default();
        state.start_thinking();
        let mut scroll = ScrollState::default();
        let theme = state.theme();
        let backend = TestBackend::new(20, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 20, 8);
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        state.transcript_cache.take_work_stats();
        let id = state.transcript.nodes()[0].id().clone();
        state.transcript.get_mut(&id).unwrap().item = DisplayItem::Card(ContentCard {
            id: id.clone(),
            unit: None,
            header: None,
            content: "x".repeat(80),
            role: CardRole::User,
            tone: DisplayTone::Normal,
            horizontal_padding: state.config.user_input_padding,
            copy_source: "x".repeat(80),
        });
        state.transcript.touch(&id);
        state.transcript_cache.mark_message_dirty(0);
        terminal
            .draw(|frame| {
                render_transcript(frame, area, &mut state, &mut scroll, &theme, false, None)
            })
            .unwrap();
        let work = state.transcript_cache.take_work_stats();
        assert_eq!(work.rebuilds, 1);
        assert_eq!(work.patches, 0);
        assert!(state.transcript_cache.valid);
    }

    #[test]
    fn wrapped_layout_scrolls_in_exact_display_rows_and_reflows_on_resize() {
        use ratatui::backend::TestBackend;

        let mut state = AppState::default();
        state.msgs.push(Msg::Assistant {
            text: "你".repeat(200),
            lines: vec![RenderLine {
                line: Line::from("你".repeat(200)),
                unit: 1,
                raw_line: Some(0),
                atomic: false,
                fill: false,
            }],
            unit_start: 1,
        });
        let theme = state.theme();
        let mut scroll = ScrollState::default();
        let backend = TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_transcript(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 40, 8),
                    &mut state,
                    &mut scroll,
                    &theme,
                    false,
                    None,
                )
            })
            .unwrap();
        let wide_rows = state.transcript_cache.display_len();
        scroll.follow = false;
        scroll.offset = 0;
        scroll_lines(&mut scroll, 4, wide_rows, false, 3);
        assert_eq!(scroll.offset, 3, "one wheel notch = three display rows");

        let backend = TestBackend::new(20, 12);
        let mut narrow = ratatui::Terminal::new(backend).unwrap();
        narrow
            .draw(|frame| {
                render_transcript(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 20, 8),
                    &mut state,
                    &mut scroll,
                    &theme,
                    false,
                    None,
                )
            })
            .unwrap();
        assert_eq!(state.transcript_cache.width, 20);
        assert!(state.transcript_cache.display_len() > wide_rows);
        let work = state.transcript_cache.take_work_stats();
        assert!(
            work.materialized_rows <= 16,
            "only viewport-scale rows materialized"
        );
    }
}

#[allow(dead_code)]
pub fn color_for(theme: &Theme, name: &str) -> Color {
    match name {
        "bg" => theme.bg,
        "fg" => theme.fg,
        _ => theme.fg,
    }
}
