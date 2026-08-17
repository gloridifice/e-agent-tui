//! Ratatui rendering: status bar, transcript, borderless input bar
//! (design §3.1, D23).

use ratatui::{
    layout::{Constraint, Layout, Position},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    cache::MessageLineRange,
    config::Theme,
    display::{
        allocate_accessories, ActivityContinuation, ActivityRow, ActivityState, CardRole,
        ContentCard, DisplayId, DisplayTone, InputAccessory, InputAccessoryKind, TranscriptBlock,
        TranscriptFormat,
    },
    input::{InputState, Suggestion},
    input_page::{FocusId, InputPage, InputPageSession, ModelPage, ThemePage},
    login::LoginState,
    model::{breathing_color, settle_color, AgentStatus, AppState, Msg, ToolState},
    runtime_command::CommandSource,
    settings::SettingsState,
};

/// Multiline input shows at most this many rows (D23).
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
    // Input Pages replace the input bar and take two thirds of the page height
    // without a floating window. The transcript keeps the top third.
    let bottom_rows = bottom_area_rows(area.height, input, input_page_open);
    let bottom = Constraint::Length(bottom_rows);
    let accessories = input_accessories(state);
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
    if let Some(question) = state.question.as_ref() {
        render_question(frame, chunks[1], question, theme);
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
    } else if let Some(question) = state.question.as_ref() {
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
    if !input_page_open && state.question.is_none() {
        if let Some(suggest) = input.suggest.as_ref() {
            render_suggest(frame, suggest, chunks[7], theme);
        }
    }
    cursor_anchor
}

fn render_info_accessory(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    label: &str,
    value: &str,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("  {label}: "),
                Style::default().fg(theme.selection).bg(theme.bg),
            ),
            Span::styled(
                trim_to_width(
                    value,
                    area.width.saturating_sub((label.len() + 4) as u16) as usize,
                ),
                Style::default().fg(theme.dim).bg(theme.bg),
            ),
        ]))
        .style(Style::default().bg(theme.bg)),
        area,
    );
}

fn render_todo(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    todos: &[(String, String)],
    theme: &Theme,
) {
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);
    if area.height == 0 {
        return;
    }
    let mut rows = vec![Line::from(Span::styled(
        "  Todo",
        Style::default()
            .fg(theme.selection)
            .bg(theme.bg)
            .add_modifier(Modifier::BOLD),
    ))];
    let visible = usize::from(area.height.saturating_sub(1));
    for (content, status) in todos.iter().take(visible) {
        let marker = match status.as_str() {
            "completed" => "✓",
            "in-progress" => "•",
            _ => "○",
        };
        rows.push(Line::from(Span::styled(
            format!(
                "  {marker} {}",
                trim_to_width(content, area.width.saturating_sub(5) as usize)
            ),
            Style::default().fg(theme.dim).bg(theme.bg),
        )));
    }
    frame.render_widget(Paragraph::new(rows), area);
}

/// Pending-prompt queue strip above the input bar: one row per prompt, Night
/// background with Bark text, `  * ` prefix; long prompts truncate to a
/// single row with `…`.
fn render_queue(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    queue: &[String],
    visible: usize,
    theme: &Theme,
) {
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);
    let truncated = queue.len() > visible;
    let shown = visible.saturating_sub(usize::from(truncated));
    let width = (area.width as usize).saturating_sub(4);
    let mut rows: Vec<Line<'static>> = queue
        .iter()
        .take(shown)
        .map(|item| {
            Line::from(Span::styled(
                format!("  * {}", trim_to_width(item, width)),
                Style::default().fg(theme.dim).bg(theme.bg),
            ))
        })
        .collect();
    if truncated {
        rows.push(Line::from(Span::styled(
            format!("  * … 还有 {} 条", queue.len() - shown),
            Style::default().fg(theme.dim).bg(theme.bg),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(rows)), area);
}

/// Slash-command suggestion popup, anchored right above the input bar:
/// borderless soft-background panel with the highlighted row following the
/// selection. `Clear` wipes the transcript cells underneath first — without
/// it, text behind the short rows would show through the panel.
fn render_suggest(
    frame: &mut Frame,
    suggest: &Suggestion,
    input_area: ratatui::layout::Rect,
    theme: &Theme,
) {
    // Plugin registries are unbounded. Keep the popup inside the viewport and
    // scroll its visible window around the selected row while retaining every
    // completion in `suggest.matches` for keyboard navigation.
    const MAX_VISIBLE_ROWS: usize = 12;
    let available_rows = input_area.y.saturating_sub(2).max(1) as usize;
    let visible_rows = suggest
        .matches
        .len()
        .min(MAX_VISIBLE_ROWS)
        .min(available_rows);
    let start = suggest
        .sel
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(suggest.matches.len().saturating_sub(visible_rows));
    let width = 46u16.min(input_area.width);
    let rect = ratatui::layout::Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(visible_rows as u16 + 2),
        width,
        height: visible_rows as u16 + 2,
    };
    frame.render_widget(ratatui::widgets::Clear, rect);
    let panel = theme.overlay.background.style();

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("❯ ", theme.overlay.accent.style()),
        Span::styled(
            if suggest.modes { "模式" } else { "命令" },
            theme.overlay.muted.style(),
        ),
    ]));
    for (i, cmd) in suggest
        .matches
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
    {
        let selected = i == suggest.sel;
        let integrated = suggest.sources.get(i).copied() == Some(CommandSource::Integrated);
        let desc = suggest
            .descriptions
            .get(i)
            .map(String::as_str)
            .unwrap_or("");
        let row_style = if selected {
            theme.overlay.selection.style()
        } else {
            theme.overlay.text.style()
        };
        let marker_style = if selected {
            row_style
        } else {
            theme.overlay.border.style()
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "❯ " } else { "  " },
                theme.overlay.accent.style(),
            ),
            Span::styled(if integrated { "↳ " } else { "" }, marker_style),
            Span::styled(cmd.clone(), row_style),
            Span::styled(
                format!("  {desc}"),
                if selected {
                    row_style
                } else {
                    theme.overlay.muted.style()
                },
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "↑↓ 选择 · Enter 发送 · Esc 关闭 · ↳ 插件命令",
        theme.overlay.muted.style(),
    )));
    frame.render_widget(Paragraph::new(Text::from(lines)).style(panel), rect);
}

/// Pending approval card: fixed above the input bar (design §4.4).
fn render_approval(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    approval: &crate::model::ApprovalCard,
    theme: &Theme,
) {
    let title = Line::from(vec![
        Span::styled(
            "⚠ 审批",
            Style::default()
                .fg(theme.running)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(theme.dim)),
        Span::styled(approval.tool_name.clone(), Style::default().fg(theme.fg)),
    ]);
    let mut rows = vec![title];
    if !approval.reason.is_empty() {
        rows.push(Line::from(Span::styled(
            approval.reason.chars().take(100).collect::<String>(),
            Style::default().fg(theme.dim),
        )));
    }
    rows.push(Line::from(vec![
        Span::styled("[Y] 允许", Style::default().fg(theme.ok)),
        Span::styled("   ", Style::default().fg(theme.dim)),
        Span::styled("[n] 拒绝", Style::default().fg(theme.err)),
        Span::styled("   [i] 详情", Style::default().fg(theme.dim)),
    ]));
    let block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(theme.running))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(Text::from(rows)), inner);
}

/// Pending user-question panel: fixed above the input bar (design §4.4).
/// Shows the current question, and the description of the highlighted option.
fn render_question(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    batch: &crate::model::QuestionBatch,
    theme: &Theme,
) {
    let current = &batch.questions[batch.current];
    let header = current
        .header
        .as_deref()
        .filter(|h| !h.is_empty())
        .unwrap_or("问题");
    let title = Line::from(vec![
        Span::styled(
            "❓ ",
            Style::default().fg(theme.user).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            header,
            Style::default().fg(theme.user).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ({}/{})", batch.current + 1, batch.questions.len()),
            Style::default().fg(theme.dim),
        ),
    ]);
    let mut rows = vec![title];
    rows.push(Line::from(Span::styled(
        current.question.clone(),
        Style::default().fg(theme.fg),
    )));
    let detail = if batch.is_free_text() {
        "无预设选项 · 直接输入文本，Enter 提交".to_string()
    } else {
        batch
            .current_options()
            .get(batch.sel)
            .and_then(|o| o.description.clone())
            .unwrap_or_else(|| "←→ 切换选项 · Enter 选中".to_string())
    };
    rows.push(Line::from(Span::styled(
        detail,
        Style::default().fg(theme.dim),
    )));
    // Borderless soft-background panel, matching the input bar's look.
    let panel = Block::default().style(Style::default().bg(theme.bg_soft));
    frame.render_widget(panel, area);
    frame.render_widget(Paragraph::new(Text::from(rows)), area);
}

/// Selection bar replacing the input bar while a question pends: `◄ A · B · C ►`
/// with the highlighted option reversed, or a `❯` draft line for free-text
/// questions. The right side carries the key hints.
fn render_question_bar(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    batch: &crate::model::QuestionBatch,
    theme: &Theme,
    padding: u16,
) -> Option<Position> {
    let block = Block::default()
        .style(Style::default().bg(theme.bg_soft))
        .padding(Padding::new(padding, padding, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let hint = format!(
        "{}/{} Enter {} · Esc 取消",
        batch.current + 1,
        batch.questions.len(),
        if batch.current + 1 < batch.questions.len() {
            "下一项"
        } else {
            "确定"
        }
    );
    let options = batch.current_options();
    let left: Line<'static>;
    if options.is_empty() {
        left = Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled(batch.draft.clone(), Style::default().fg(theme.fg)),
            // Software cursor: the hardware cursor stays hidden during all
            // terminal diff writes to avoid jumping through animated rows.
            Span::styled(" ", Style::default().fg(theme.bg).bg(theme.fg)),
        ]);
    } else {
        let mut spans = Vec::new();
        spans.push(Span::styled("◄ ", Style::default().fg(theme.dim)));
        for (i, option) in options.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
            }
            if i == batch.sel {
                spans.push(Span::styled(
                    option.label.clone(),
                    Style::default().fg(theme.bg).bg(theme.fg),
                ));
            } else {
                spans.push(Span::styled(
                    option.label.clone(),
                    Style::default().fg(theme.fg),
                ));
            }
        }
        spans.push(Span::styled(" ►", Style::default().fg(theme.dim)));
        left = Line::from(spans);
    }
    // Render through the buffer directly: no wrapping, hard clip at edges.
    let buffer = frame.buffer_mut();
    buffer.set_line(inner.x, inner.y, &left, inner.width);
    let hint_line = Line::from(Span::styled(hint, Style::default().fg(theme.dim)))
        .alignment(ratatui::layout::Alignment::Right);
    // +4 slack: Line::width() under-counts CJK by a couple of cells vs the
    // buffer writer (same allowance as the status bar).
    let hint_width = (hint_line.width() + 4).min(inner.width as usize) as u16;
    let hint_x = inner.x + inner.width.saturating_sub(hint_width);
    buffer.set_line(hint_x, inner.y, &hint_line, hint_width);
    if options.is_empty() {
        // Keep the hidden terminal cursor anchored to the draft so IME
        // candidate windows still open beside the software cursor.
        let col = unicode_width::UnicodeWidthStr::width(batch.draft.as_str()) as u16;
        Some(Position::new(inner.x + 2 + col, inner.y))
    } else {
        None
    }
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
fn render_status(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &AppState,
    _scroll: &ScrollState,
    theme: &Theme,
) {
    let dim = theme.input.status_hint.style();
    // Running bullet leads the status bar: yellow breathing while the agent
    // is running (or has just been sent work), gray while idle. One space
    // separates it from the elements that follow.
    let bullet = if state.status == AgentStatus::Running || state.working {
        Span::styled(
            "•",
            Style::default().fg(breathing_color(theme, state.breath_phase())),
        )
    } else {
        Span::styled("•", dim)
    };
    let mode = state
        .current_mode
        .as_deref()
        .unwrap_or(state.config.default_mode.as_str());
    let mut left_spans = vec![
        bullet,
        Span::styled(" ", dim),
        Span::styled(mode.to_owned(), dim),
    ];
    if let Some(model) = state
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
    {
        left_spans.push(Span::styled(" ", dim));
        left_spans.push(Span::styled(model.to_owned(), dim));
    }
    if let Some(rate) = state.cache_hit_rate() {
        left_spans.push(Span::styled(" ", dim));
        left_spans.push(Span::styled(format!("CH{rate}%"), dim));
    }
    let left = Line::from(left_spans);
    let right =
        Line::from(Span::styled("^h Help", dim)).alignment(ratatui::layout::Alignment::Right);
    // Render through the buffer directly: no wrapping, hard clip at edges.
    let buffer = frame.buffer_mut();
    buffer.set_line(area.x, area.y, &left, area.width);
    // +4 slack: Line::width() under-counts CJK by a couple of cells vs the
    // buffer writer; without it the last characters would be clipped.
    let right_width = (right.width() + 4).min(area.width as usize) as u16;
    let right_x = area.x + area.width.saturating_sub(right_width);
    buffer.set_line(right_x, area.y, &right, right_width);
}

/// Session title row below the status bar: the latest `session/title` of
/// the attached session on the left and the session's workspace path on the
/// right. Overly long titles truncate with an ellipsis so the path stays
/// visible; the row is blank until the session has either.
fn render_title(frame: &mut Frame, area: ratatui::layout::Rect, state: &AppState, theme: &Theme) {
    let style = theme.input.status_hint.style();
    frame.render_widget(ratatui::widgets::Clear, area);
    let buffer = frame.buffer_mut();
    let width = area.width as usize;

    // Left-aligned title, truncated to leave the path (plus a small gap)
    // visible. Drawn first so the path below wins any overlap (defensive:
    // the truncation already reserves the path's columns).
    let title = match state.session_title.as_deref() {
        Some(title) if !title.trim().is_empty() => title.trim().to_owned(),
        _ => "新会话".to_owned(),
    };
    let cwd = state
        .session_cwd
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let path_w = UnicodeWidthStr::width(cwd.as_str());
    let avail = width.saturating_sub(path_w.saturating_add(2));
    let shown = if UnicodeWidthStr::width(title.as_str()) > avail {
        trim_to_width(&title, avail)
    } else {
        title
    };
    // `Buffer::set_line` does not pad a short line to the supplied width.
    // Write explicit trailing cells so a shorter title cannot leave glyphs
    // from the previous frame behind (especially after CJK wide cells).
    let shown_width = UnicodeWidthStr::width(shown.as_str());
    let padded = format!("{shown}{}", " ".repeat(width.saturating_sub(shown_width)));
    let left = Line::from(Span::styled(padded, style));
    buffer.set_line(area.x, area.y, &left, area.width);

    // Right-aligned workspace path (the session's header cwd), positioned by
    // its display width so it sits flush against the right edge. `set_line`
    // ignores `Line::alignment`, so the x offset is computed here instead.
    if !cwd.is_empty() {
        let right = Line::from(Span::styled(cwd, style));
        let right_x = area.x + area.width.saturating_sub(path_w as u16);
        buffer.set_line(right_x, area.y, &right, path_w as u16);
    }
}

/// First-seen-ordered per-file counts over FULL paths (dedupe happens
/// before basenames, so two same-named files in different directories stay
/// distinct); the display shows the basename. Repeated reads/edits of one
/// file collapse into `name xN`.
fn counted_files(files: &[String]) -> Vec<(String, usize)> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for f in files {
        if !counts.contains_key(f) {
            order.push(f.clone());
        }
        *counts.entry(f.clone()).or_insert(0) += 1;
    }
    order
        .into_iter()
        .map(|f| {
            let n = counts[&f];
            let name = f
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(f.as_str())
                .to_string();
            (name, n)
        })
        .collect()
}

/// `foo.rs x2, bar.rs` — the `xN` suffix appears only for repeats.
fn file_list(files: &[(String, usize)]) -> String {
    files
        .iter()
        .map(|(f, n)| {
            if *n > 1 {
                format!("{f} x{n}")
            } else {
                f.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn activity_row_line(
    row: &ActivityRow,
    state: &AppState,
    color_override: Option<Color>,
) -> Line<'static> {
    let theme = state.theme();
    let color = color_override.unwrap_or_else(|| match row.state {
        ActivityState::Waiting => theme.working_status.waiting.fg,
        ActivityState::Running => breathing_color(&theme, state.breath_phase()),
        ActivityState::Success => theme.working_status.success.fg,
        ActivityState::Failure => theme.working_status.failure.fg,
        ActivityState::Cancelled => theme.working_status.cancelled.fg,
    });
    let mut spans = vec![
        Span::styled(
            " ".repeat(2 + usize::from(row.depth) * 2),
            theme.activity.detail.style(),
        ),
        Span::styled("•", Style::default().fg(color)),
        Span::styled(" ", theme.activity.detail.style()),
        Span::styled(row.label.clone(), theme.activity.label.style()),
    ];
    if !row.summary.is_empty() {
        spans.push(Span::styled(" ", theme.activity.detail.style()));
        spans.push(Span::styled(
            row.summary.clone(),
            theme.activity.detail.style(),
        ));
    }
    for continuation in &row.continuations {
        spans.push(Span::styled(
            continuation.separator.clone(),
            theme.activity.detail.style(),
        ));
        spans.push(Span::styled(
            continuation.label.clone(),
            theme.activity.label.style(),
        ));
        if !continuation.summary.is_empty() {
            spans.push(Span::styled(" ", theme.activity.detail.style()));
            spans.push(Span::styled(
                continuation.summary.clone(),
                theme.activity.detail.style(),
            ));
        }
    }
    if row.count > 1 {
        spans.push(Span::styled(
            format!(" x{}", row.count),
            theme.activity.metadata.style(),
        ));
    }
    if state.config.show_tool_duration {
        if let Some(duration_ms) = row.duration_ms {
            spans.push(Span::styled(
                format!(" · {:.1}s", duration_ms as f64 / 1000.0),
                theme.activity.metadata.style(),
            ));
        }
    }
    Line::from(spans)
}

fn transcript_block_lines(block: &TranscriptBlock, state: &AppState) -> Vec<Line<'static>> {
    let theme = state.theme();
    let color = match block.tone {
        DisplayTone::Normal => theme.surface.primary_text.fg,
        DisplayTone::Dim => theme.surface.muted_text.fg,
        DisplayTone::Info => theme.log.info.fg,
        DisplayTone::Warning => theme.log.warning.fg,
        DisplayTone::Error => theme.log.error.fg,
    };
    let prefix = if block.tone == DisplayTone::Error {
        "✗ "
    } else {
        ""
    };
    block
        .content
        .lines()
        .take(MAX_RENDER_LINES_PER_MSG)
        .map(|line| {
            Line::from(Span::styled(
                format!("{prefix}{line}"),
                Style::default().fg(color),
            ))
        })
        .collect()
}

fn content_card_lines(
    card: &ContentCard,
    state: &AppState,
    area_width: usize,
) -> Vec<Line<'static>> {
    let theme = state.theme();
    let gutter = card.horizontal_padding.min(area_width);
    let avail = area_width.saturating_sub(gutter).max(1);
    let style = match card.role {
        CardRole::User => theme.card.user,
        CardRole::Context => theme.card.context,
        CardRole::Detail => theme.card.detail,
        CardRole::Attachment => theme.card.attachment,
    };
    let fg = style.fg;
    let bg = style.bg.unwrap_or(theme.bg_soft);
    let fill_row = || {
        Line::from(Span::styled(
            " ".repeat(area_width),
            Style::default().fg(fg).bg(bg),
        ))
    };
    let mut out = vec![fill_row()];
    if let Some(header) = &card.header {
        let mut row = Line::from(vec![
            Span::styled(" ".repeat(gutter), Style::default().fg(fg).bg(bg)),
            Span::styled(
                header.clone(),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            ),
        ]);
        let used = row.width();
        if used < area_width {
            row.push_span(Span::styled(
                " ".repeat(area_width - used),
                Style::default().fg(fg).bg(bg),
            ));
        }
        out.push(row);
    }
    for line in card.content.lines() {
        if line.is_empty() {
            out.push(fill_row());
            continue;
        }
        for chunk in wrap_text(line, avail) {
            let mut row = Line::from(vec![
                Span::styled(" ".repeat(gutter), Style::default().fg(fg).bg(bg)),
                Span::styled(chunk, Style::default().fg(fg).bg(bg)),
            ]);
            let used = row.width();
            if used < area_width {
                row.push_span(Span::styled(
                    " ".repeat(area_width - used),
                    Style::default().fg(fg).bg(bg),
                ));
            }
            out.push(row);
        }
    }
    out.push(fill_row());
    out
}

fn msg_lines(msg: &Msg, state: &AppState) -> Vec<Line<'static>> {
    let theme = state.theme();
    match msg {
        // Thinking output collapses into the breathing `• Thinking... xN`
        // row; `is_hidden_msg` also skips it in the cache and copy layout.
        Msg::Block(_) if is_hidden_msg(msg) => Vec::new(),
        Msg::Block(block) => transcript_block_lines(block, state),
        // Cards are built width-aware in styled_msg_lines.
        Msg::Card(_) => Vec::new(),
        Msg::Activity(row) => vec![activity_row_line(row, state, None)],
        // User blocks are built width-aware (wrapped with the gutter on
        // every row) in styled_msg_lines.
        Msg::User { .. } => Vec::new(),
        Msg::Assistant { lines, .. } => lines.iter().map(|r| r.line.clone()).collect(),
        Msg::Streaming { text } => {
            let block = TranscriptBlock {
                id: DisplayId::correlated("assistant", "streaming"),
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: text.clone(),
                streaming: true,
            };
            let mut lines = transcript_block_lines(&block, state);
            // Breathing `•` on the tail while the model streams.
            if let Some(last) = lines.last_mut() {
                last.push_span(Span::styled(" ", Style::default().fg(theme.dim)));
                last.push_span(Span::styled(
                    "•",
                    Style::default().fg(breathing_color(&theme, state.breath_phase())),
                ));
            }
            lines
        }
        Msg::Tool(card) => {
            let (activity_state, summary, duration_ms, color) = match &card.state {
                ToolState::Running => (ActivityState::Running, card.summary.clone(), None, None),
                ToolState::Done {
                    ok,
                    lines,
                    lines_truncated,
                    duration_ms,
                } => {
                    let target = if *ok { theme.ok } else { theme.err };
                    let color = match (&card.done_since, &card.done_from) {
                        (Some(since), Some(from)) => settle_color(*from, target, since.elapsed()),
                        _ => target,
                    };
                    let concise_create = card.name == "create";
                    (
                        if *ok {
                            ActivityState::Success
                        } else {
                            ActivityState::Failure
                        },
                        if concise_create {
                            card.summary.clone()
                        } else if *lines_truncated {
                            format!("{} · 末尾 {lines} 行", card.summary)
                        } else {
                            format!("{} · {lines} 行", card.summary)
                        },
                        (!concise_create).then_some(*duration_ms),
                        Some(color),
                    )
                }
            };
            let mut row = ActivityRow::root(
                DisplayId::correlated("tool", &card.call_id),
                card.name.clone(),
            );
            row.summary = summary;
            row.state = activity_state;
            row.start_ms = Some(card.start_ms);
            row.duration_ms = duration_ms;
            vec![activity_row_line(&row, state, color)]
        }
        Msg::Thinking(card) => {
            let (activity_state, color) = match card.state {
                crate::model::ThinkState::Running => (ActivityState::Running, None),
                crate::model::ThinkState::Done => {
                    let color = match (&card.done_since, &card.done_from) {
                        (Some(since), Some(from)) => settle_color(*from, theme.ok, since.elapsed()),
                        _ => theme.ok,
                    };
                    (ActivityState::Success, Some(color))
                }
            };
            let mut row =
                ActivityRow::root(DisplayId::correlated("thinking", "current"), "Thinking...");
            row.state = activity_state;
            row.count = card.count;
            vec![activity_row_line(&row, state, color)]
        }
        Msg::FileGroup(group) => {
            use crate::model::FileAction;

            let files_for = |action: FileAction, ok: Option<bool>| -> Vec<String> {
                group
                    .items
                    .iter()
                    .filter(|item| item.action == action && item.ok == ok)
                    .map(|item| item.file.clone())
                    .collect()
            };
            let group_id = group
                .items
                .first()
                .map(|item| item.call_id.as_str())
                .unwrap_or("group");
            let make_row =
                |suffix: &str, label: &str, summary: String, activity_state: ActivityState| {
                    let mut row = ActivityRow::root(
                        DisplayId::correlated("file-group", &format!("{group_id}:{suffix}")),
                        label,
                    );
                    row.summary = summary;
                    row.state = activity_state;
                    row
                };
            let combined_row = |suffix: &str,
                                actions: &[FileAction],
                                ok: Option<bool>,
                                activity_state: ActivityState,
                                trailing_semicolon: bool|
             -> Option<ActivityRow> {
                let mut parts = actions.iter().filter_map(|action| {
                    let files = files_for(*action, ok);
                    (!files.is_empty()).then(|| (action.label(), file_list(&counted_files(&files))))
                });
                let (label, summary) = parts.next()?;
                let mut row = make_row(suffix, label, summary, activity_state);
                for (label, summary) in parts {
                    row.continuations.push(ActivityContinuation {
                        separator: "; ".into(),
                        label: label.into(),
                        summary,
                    });
                }
                if trailing_semicolon {
                    if let Some(last) = row.continuations.last_mut() {
                        last.summary.push(';');
                    } else {
                        row.summary.push(';');
                    }
                }
                Some(row)
            };
            let push_failures = |actions: &[FileAction], out: &mut Vec<Line<'static>>| {
                for action in actions {
                    let files = files_for(*action, Some(false));
                    for (index, (name, count)) in counted_files(&files).into_iter().enumerate() {
                        let summary = if count > 1 {
                            format!("{name} x{count}")
                        } else {
                            name
                        };
                        let row = make_row(
                            &format!("{}-failed:{index}", action.label()),
                            action.label(),
                            summary,
                            ActivityState::Failure,
                        );
                        out.push(activity_row_line(&row, state, None));
                    }
                }
            };

            let read_actions = [FileAction::Read, FileAction::View];
            let write_actions = [FileAction::Edit, FileAction::Replace, FileAction::Insert];
            let pending_reads = group
                .items
                .iter()
                .any(|item| item.action.is_read_like() && item.ok.is_none());
            let pending_writes = group
                .items
                .iter()
                .any(|item| !item.action.is_read_like() && item.ok.is_none());
            let mut lines = Vec::new();
            if pending_reads {
                if let Some(row) = combined_row(
                    "read-running",
                    &read_actions,
                    None,
                    ActivityState::Running,
                    false,
                ) {
                    lines.push(activity_row_line(&row, state, None));
                }
                push_failures(&read_actions, &mut lines);
            } else if pending_writes {
                if let Some(row) = combined_row(
                    "read-done",
                    &read_actions,
                    Some(true),
                    ActivityState::Success,
                    true,
                ) {
                    lines.push(activity_row_line(&row, state, None));
                }
                push_failures(&read_actions, &mut lines);
                if let Some(row) = combined_row(
                    "write-running",
                    &write_actions,
                    None,
                    ActivityState::Running,
                    false,
                ) {
                    lines.push(activity_row_line(&row, state, None));
                }
                push_failures(&write_actions, &mut lines);
            } else {
                if let Some(row) = combined_row(
                    "done",
                    &FileAction::FOLD_ORDER,
                    Some(true),
                    ActivityState::Success,
                    false,
                ) {
                    let color = match (&group.done_since, &group.done_from) {
                        (Some(since), Some(from)) => {
                            Some(settle_color(*from, theme.ok, since.elapsed()))
                        }
                        _ => None,
                    };
                    lines.push(activity_row_line(&row, state, color));
                }
                push_failures(&FileAction::FOLD_ORDER, &mut lines);
            }
            lines
        }
        Msg::System { text } => transcript_block_lines(
            &TranscriptBlock {
                id: DisplayId::correlated("system", "legacy"),
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Dim,
                copy_source: text.clone(),
                streaming: false,
            },
            state,
        ),
        Msg::Error { text } => transcript_block_lines(
            &TranscriptBlock {
                id: DisplayId::correlated("error", "legacy"),
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Error,
                copy_source: text.clone(),
                streaming: false,
            },
            state,
        ),
    }
}

/// Tool cards, Thinking rows, and read/edit file groups are "activity"
/// rows: consecutive ones render glued together with no gap row between them.
fn is_activity_msg(msg: &Msg) -> bool {
    matches!(
        msg,
        Msg::Activity(_) | Msg::Tool(_) | Msg::FileGroup(_) | Msg::Thinking(_)
    )
}

/// Thinking output (reasoning blocks) is collapsed into the breathing
/// `• Thinking... xN` row: it contributes zero transcript rows and zero
/// inter-message gap, so the cache builder and copy provenance must skip it.
fn is_hidden_msg(msg: &Msg) -> bool {
    matches!(
        msg,
        Msg::Block(block) if block.format == TranscriptFormat::Reasoning
    )
}

/// Whether the next visible message is another activity row. Hidden messages
/// are transparent to layout adjacency, just as they are to rendering/copy.
fn next_visible_is_activity(msgs: &[Msg], index: usize) -> bool {
    msgs.iter()
        .skip(index + 1)
        .find(|msg| !is_hidden_msg(msg))
        .is_some_and(is_activity_msg)
}

trait WrapSink {
    fn segment(&mut self, text: &str, style: Style);
    fn end_row(&mut self, base: Style);
}

#[derive(Default)]
struct CountWrapSink {
    rows: usize,
}

impl WrapSink for CountWrapSink {
    fn segment(&mut self, _text: &str, _style: Style) {}

    fn end_row(&mut self, _base: Style) {
        self.rows += 1;
    }
}

#[derive(Default)]
struct LineWrapSink {
    current: Vec<Span<'static>>,
    rows: Vec<Line<'static>>,
}

impl WrapSink for LineWrapSink {
    fn segment(&mut self, text: &str, style: Style) {
        if !text.is_empty() {
            self.current.push(Span::styled(text.to_owned(), style));
        }
    }

    fn end_row(&mut self, base: Style) {
        self.rows
            .push(Line::from(std::mem::take(&mut self.current)).patch_style(base));
    }
}

/// One linear grapheme/display-width scan shared by row counting and
/// materializing. Flattening span text for segmentation keeps combining marks
/// and emoji ZWJ sequences together even when a style boundary bisects one.
fn scan_wrapped<S: WrapSink>(line: &Line<'static>, width: usize, sink: &mut S) {
    let base = line.style;
    let mut text = String::new();
    let mut styles = Vec::with_capacity(line.spans.len());
    for span in &line.spans {
        let start = text.len();
        text.push_str(span.content.as_ref());
        styles.push((start, text.len(), span.style));
    }
    let emit = |start: usize, end: usize, sink: &mut S| {
        for (style_start, style_end, style) in &styles {
            let from = start.max(*style_start);
            let to = end.min(*style_end);
            if from < to {
                sink.segment(&text[from..to], *style);
            }
        }
    };

    let mut used = 0usize;
    let mut have = false;
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if grapheme_width > width {
            if have {
                sink.end_row(base);
            }
            emit(start, end, sink);
            sink.end_row(base);
            used = 0;
            have = false;
            continue;
        }
        if have && used + grapheme_width > width {
            sink.end_row(base);
            used = 0;
        }
        emit(start, end, sink);
        used += grapheme_width;
        have = true;
        if used == width {
            sink.end_row(base);
            used = 0;
            have = false;
        }
    }
    if have {
        sink.end_row(base);
    }
}

/// Split one line into wrapped rows without Ratatui's exact-width phantom row.
fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width {
        return vec![line];
    }
    let mut sink = LineWrapSink::default();
    scan_wrapped(&line, width, &mut sink);
    if sink.rows.is_empty() {
        vec![Line::default().patch_style(line.style)]
    } else {
        sink.rows
    }
}

fn wrapped_rows(line: &Line<'static>, width: usize) -> usize {
    if width == 0 || line.width() <= width {
        return 1;
    }
    let mut sink = CountWrapSink::default();
    scan_wrapped(line, width, &mut sink);
    sink.rows.max(1)
}

/// Keep an activity card on one display row. Truncation happens here, where
/// `width` is the resolved page width (including `page_max_width`), rather
/// than earlier in the model where only the terminal-sized payload is known.
fn truncate_activity_line(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    let base = line.style;
    if width == 0 {
        return Line::default().patch_style(base);
    }

    let budget = width - 1; // reserve one display column for `…`
    let mut used = 0usize;
    let mut spans = Vec::new();
    let mut ellipsis_style = Style::default();
    'outer: for span in line.spans {
        let mut kept = String::new();
        ellipsis_style = span.style;
        for ch in span.content.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + char_width > budget {
                if !kept.is_empty() {
                    spans.push(Span::styled(kept, span.style));
                }
                break 'outer;
            }
            kept.push(ch);
            used += char_width;
        }
        if !kept.is_empty() {
            spans.push(Span::styled(kept, span.style));
        }
    }
    spans.push(Span::styled("…", ellipsis_style));
    Line::from(spans).patch_style(base)
}

/// One message rendered to transcript lines, including the full-width soft
/// background of user blocks and of `fill`-flagged code/mermaid rows. Shared
/// by the full cache rebuild and the incremental tail splice so both produce
/// identical rows.
fn styled_msg_lines(msg: &Msg, state: &AppState, area_width: usize) -> Vec<Line<'static>> {
    let theme = state.theme();
    if let Msg::Assistant { lines, .. } = msg {
        // Assistant lines carry per-row fill flags (code/mermaid blocks);
        // those fill with the Night background (#2b292d, the bg slot).
        return lines
            .iter()
            .map(|r| {
                let mut line = r.line.clone();
                if r.fill {
                    let fill_style = theme.markdown.code_background.style();
                    line = line.patch_style(fill_style);
                    let width = line.width();
                    if width < area_width {
                        line.push_span(Span::styled(" ".repeat(area_width - width), fill_style));
                    }
                }
                line
            })
            .collect();
    }
    if let Msg::Card(card) = msg {
        return content_card_lines(card, state, area_width);
    }
    if let Msg::User { text } = msg {
        let card = ContentCard {
            id: DisplayId::correlated("user", "legacy"),
            unit: None,
            header: None,
            content: text.clone(),
            role: CardRole::User,
            tone: DisplayTone::Normal,
            horizontal_padding: state.config.user_input_padding,
            copy_source: text.clone(),
        };
        return content_card_lines(&card, state, area_width);
    }
    let lines = msg_lines(msg, state);
    if is_activity_msg(msg) {
        lines
            .into_iter()
            .map(|line| truncate_activity_line(line, area_width))
            .collect()
    } else {
        lines
    }
}

/// Copy/navigation provenance derived from the exact same message layout used
/// to build the transcript cache. This is the single owner of padding, wrapping,
/// and inter-message gap decisions.
pub struct CopyLayoutRow {
    pub unit: u64,
    pub raw_line: Option<usize>,
    pub atomic: bool,
    pub text: String,
    pub global_row: usize,
}

pub fn copy_layout_rows(state: &AppState) -> Vec<CopyLayoutRow> {
    let mut rows = Vec::new();
    let mut global_row = 0usize;
    let width = state.transcript_cache.width.max(1);
    for (index, msg) in state.msgs.iter().enumerate() {
        if is_hidden_msg(msg) {
            continue;
        }
        let layout_lines = styled_msg_lines(msg, state, width);
        if let Msg::Assistant { lines, .. } = msg {
            for (render_line, layout_line) in lines.iter().zip(layout_lines.iter()) {
                for wrapped in wrap_line(layout_line.clone(), width) {
                    rows.push(CopyLayoutRow {
                        unit: render_line.unit,
                        raw_line: render_line.raw_line,
                        atomic: render_line.atomic,
                        text: wrapped
                            .spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect(),
                        global_row,
                    });
                    global_row += 1;
                }
            }
        } else if let Some(unit) = match msg {
            Msg::Block(block) => block.unit,
            Msg::Card(card) => card.unit,
            _ => None,
        } {
            for (raw_line, line) in layout_lines.iter().enumerate() {
                for wrapped in wrap_line(line.clone(), width) {
                    rows.push(CopyLayoutRow {
                        unit,
                        raw_line: Some(raw_line),
                        atomic: false,
                        text: wrapped
                            .spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect(),
                        global_row,
                    });
                    global_row += 1;
                }
            }
        } else {
            global_row += layout_lines
                .iter()
                .map(|line| wrapped_rows(line, width))
                .sum::<usize>();
        }
        let next_is_activity = next_visible_is_activity(&state.msgs, index);
        if !(is_activity_msg(msg) && next_is_activity) {
            global_row += 1;
        }
    }
    rows
}

fn rebuild_transcript_cache(state: &mut AppState, width: usize) {
    let _zone = crate::tracy_zone!("transcript rebuild");
    let mut base = Vec::new();
    let mut ranges = vec![None; state.msgs.len()];
    let mut tail_len = 0usize;
    for (index, msg) in state.msgs.iter().enumerate() {
        if is_hidden_msg(msg) {
            continue;
        }
        let start = base.len();
        let lines = styled_msg_lines(msg, state, width);
        let line_count = lines.len();
        base.extend(lines);
        let next_is_activity = next_visible_is_activity(&state.msgs, index);
        let gap = !(is_activity_msg(msg) && next_is_activity);
        ranges[index] = Some(MessageLineRange {
            start,
            end: start + line_count,
            owns_gap: gap,
        });
        tail_len = line_count + usize::from(gap);
        if gap {
            base.push(Line::default());
        }
    }
    let cache = &mut state.transcript_cache;
    cache.lines = base;
    cache.message_ranges = ranges;
    cache.tail_len = tail_len;
    cache.valid = true;
    cache.tail_dirty = false;
    cache.dirty_messages.clear();
    cache.structural_rebuilt();
}

fn refresh_transcript_cache(state: &mut AppState, width: usize) {
    if state.transcript_cache.width != width {
        state.transcript_cache.width = width;
        state.transcript_cache.invalidate();
    }
    crate::presentation::materialize_assistants(state);
    if !state.transcript_cache.valid {
        rebuild_transcript_cache(state, width);
        return;
    }

    if state.transcript_cache.tail_dirty {
        let keep = state
            .transcript_cache
            .lines
            .len()
            .saturating_sub(state.transcript_cache.tail_len);
        let last_index = state.msgs.len().checked_sub(1);
        let rendered = last_index.map(|index| styled_msg_lines(&state.msgs[index], state, width));
        let cache = &mut state.transcript_cache;
        cache.lines.truncate(keep);
        if let (Some(index), Some(lines)) = (last_index, rendered) {
            let count = lines.len();
            cache.lines.extend(lines);
            cache.lines.push(Line::default());
            cache.tail_len = count + 1;
            if cache.message_ranges.len() != state.msgs.len() {
                cache.message_ranges.resize(state.msgs.len(), None);
            }
            cache.message_ranges[index] = Some(MessageLineRange {
                start: keep,
                end: keep + count,
                owns_gap: true,
            });
            cache.dirty_messages.remove(&index);
        } else {
            cache.tail_len = 0;
        }
        cache.tail_dirty = false;
        let tail_row_counts = cache.lines[keep..]
            .iter()
            .map(|line| wrapped_rows(line, width))
            .collect::<Vec<_>>();
        cache.structural_tail_updated(keep, width, &tail_row_counts);
    }

    if !state.transcript_cache.dirty_messages.is_empty() {
        let indices = state
            .transcript_cache
            .dirty_messages
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let patches = indices
            .iter()
            .filter_map(|index| {
                state
                    .msgs
                    .get(*index)
                    .map(|msg| (*index, styled_msg_lines(msg, state, width)))
            })
            .collect::<Vec<_>>();
        let mut fallback = false;
        let mut applied = 0usize;
        for (index, lines) in patches {
            let Some(range) = state
                .transcript_cache
                .message_ranges
                .get(index)
                .and_then(|range| *range)
            else {
                fallback = true;
                break;
            };
            if range.len() != lines.len() {
                fallback = true;
                break;
            }
            state.transcript_cache.lines[range.start..range.end].clone_from_slice(&lines);
            applied += 1;
        }
        if fallback {
            rebuild_transcript_cache(state, width);
        } else {
            state.transcript_cache.dirty_messages.clear();
            state.transcript_cache.patched(applied);
        }
    }
}

fn render_transcript(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut AppState,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    overlay: Option<&CopyOverlay>,
) {
    let visible = area.height as usize;
    let width = area.width as usize;
    refresh_transcript_cache(state, width);
    state.transcript_cache.ensure_layout(width, wrapped_rows);
    // History prepend anchors are display-row totals, not unwrapped base lines.
    if let Some(anchor) = state.transcript_cache.prepend_anchor.take() {
        let delta = state
            .transcript_cache
            .layout
            .total_rows()
            .saturating_sub(anchor);
        scroll.offset = scroll.offset.saturating_add(delta);
    }
    let len = state.transcript_cache.layout.total_rows();
    let mut available = visible;
    let show_hint = !scroll.follow && scroll.offset == 0;
    if show_hint {
        available = available.saturating_sub(1).max(1);
    }
    let start = if scroll.follow {
        len.saturating_sub(available)
    } else {
        scroll.offset.min(len.saturating_sub(1))
    };
    scroll.offset = start;
    let end = start.saturating_add(available).min(len);
    let (mut base_index, _) = state.transcript_cache.layout.locate(start);
    let mut display: Vec<Line<'static>> = Vec::with_capacity(available);
    while base_index < state.transcript_cache.lines.len() && display.len() < available {
        let base_start = state.transcript_cache.layout.prefix[base_index];
        let wrapped = wrap_line(state.transcript_cache.lines[base_index].clone(), width);
        for (row_index, mut row) in wrapped.into_iter().enumerate() {
            let global_row = base_start + row_index;
            if global_row < start {
                continue;
            }
            if global_row >= end {
                break;
            }
            if let Some(overlay) = overlay {
                let mut style = Style::default();
                if overlay
                    .sel
                    .is_some_and(|(lo, hi)| global_row >= lo && global_row <= hi)
                {
                    style = style.bg(theme.selection);
                }
                if overlay.cursor_row == global_row {
                    style = style.fg(theme.bg).bg(theme.fg);
                }
                if style != Style::default() {
                    row = row.patch_style(style);
                }
            }
            // Only an explicit row-level background makes a solid row.
            // Span backgrounds (notably inline-code chips) must stay local;
            // treating any span bg as a row fill leaks that color through the
            // source separator and every trailing terminal cell.
            if let Some(bg) = row.style.bg {
                let used = row.width();
                if used < width {
                    row.push_span(Span::styled(
                        " ".repeat(width - used),
                        Style::default().fg(theme.fg).bg(bg),
                    ));
                }
            }
            display.push(row);
        }
        base_index += 1;
    }
    state
        .transcript_cache
        .record_materialized_rows(display.len());
    if help_visible {
        display.extend(help_overlay(theme));
    }
    // Lazy scroll-back hint at the top of the transcript (display-only).
    if show_hint {
        let hint = if state.history_loading {
            "（正在加载更早的消息…）"
        } else if state.history_exhausted {
            "（已到最早的消息）"
        } else {
            "（PageUp 加载更早的消息）"
        };
        display.insert(
            0,
            Line::from(Span::styled(hint, Style::default().fg(theme.dim))),
        );
    }
    let paragraph = Paragraph::new(Text::from(display)).style(Style::default().fg(theme.fg));
    frame.render_widget(paragraph, area);
}

fn help_overlay(theme: &Theme) -> Vec<Line<'static>> {
    let rows = [
        "帮助 — e",
        "Enter 发送   Shift+Enter 换行   ↑↓ 行间移动/边界切换提示词",
        "Esc 中断   Ctrl+C 清空输入/空闲退出   /exit /q /quit 退出",
        "Ctrl+B 复制模式   Ctrl+N 会话选择器   Ctrl+H 帮助",
        "输入 /：补全内置命令及当前会话自动接入的 DSH/插件命令   Tab/↑↓ 选择",
        "/settings 设置面板   /login 登录（API key/Account/Proxy）   /new [模式] 新建会话   PgUp/PgDn/滚轮滚动消息",
        "/theme 切换主题   /model 选择模型   /reload 重载配置/主题/技能   /skill:<名称> 注入技能",
        "Input Page: 方向键/hjkl 移动焦点   Enter 执行   Esc 返回；编辑时 hjkl 输入文字",
        "/resume 切换会话（打开选择器）/ /resume <会话ID> 直接切换",
        "审批: Y 允许 / n 拒绝   提问: ←→ 切换选项  Enter 选中/下一项(最后一项确定)  Esc 取消",
        "复制模式: hjkl 移动  V 行选  Ctrl+V 块选  y 复制  Esc 退出",
        "q/Esc 关闭帮助",
    ];
    rows.iter()
        .map(|r| {
            Line::from(Span::styled(
                (*r).to_string(),
                Style::default().fg(theme.fg).bg(theme.bg_soft),
            ))
        })
        .collect()
}

fn render_input(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    input: &InputState,
    theme: &Theme,
    copy_active: bool,
    toast: Option<&str>,
    padding: u16,
) -> Option<Position> {
    let block = Block::default()
        .style(theme.input.background.style())
        // Configurable horizontal gutter + 1-row vertical padding.
        .padding(Padding::new(padding, padding, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if copy_active {
        // -- COPY -- status strip replaces the editor (design §3.1).
        let hint = Line::from(vec![
            Span::styled(
                "-- COPY -- ",
                theme
                    .input
                    .status_accent
                    .style()
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[hjkl]移动 [V]行选 [Ctrl+V]块选 [y]复制 [Enter]展开 [Esc]退出",
                theme.input.status_hint.style(),
            ),
        ]);
        frame.render_widget(Paragraph::new(Text::from(vec![hint])), inner);
        return None;
    }

    if let Some(search) = &input.search {
        // Ctrl+R history search strip.
        let matches = input.matching_history(&search.query);
        let preview = matches
            .get(search.sel)
            .map(|m| m.chars().take(60).collect::<String>())
            .unwrap_or_default();
        let line = Line::from(vec![
            Span::styled("search: ", theme.input.prompt.style()),
            Span::styled(search.query.clone(), theme.input.text.style()),
            Span::styled(" ▏ ", theme.input.hint.style()),
            Span::styled(preview, theme.input.hint.style()),
            Span::styled(
                format!("  ({}/{})", search.sel + 1, matches.len()),
                theme.input.hint.style(),
            ),
        ]);
        frame.render_widget(Paragraph::new(Text::from(vec![line])), inner);
        return None;
    }

    if let Some(toast_text) = toast {
        let line = Line::from(Span::styled(
            format!("❯ {toast_text}"),
            theme.working_status.success.style(),
        ));
        frame.render_widget(Paragraph::new(Text::from(vec![line])), inner);
        return None;
    }

    let (display, placeholder) = input.display_text();
    // Wrap every display line at the inner width so long content stays
    // inside the input box; each chunk remembers its char offset within
    // `display` (= `buf` for non-placeholder content).
    let wrap_w = (inner.width as usize).max(1);
    let mut chunks: Vec<(String, usize)> = Vec::new();
    let mut offset = 0usize;
    for line in display.split('\n') {
        if line.is_empty() {
            chunks.push((String::new(), offset));
        } else {
            for chunk in wrap_text(line, wrap_w) {
                let len = chunk.chars().count();
                chunks.push((chunk, offset));
                offset += len;
            }
        }
        offset += 1; // the newline itself
    }
    if chunks.is_empty() {
        chunks.push((String::new(), 0));
    }
    // Multiline window: keep the cursor row visible within INPUT_MAX_ROWS.
    let total = chunks.len();
    let mut cursor_row = 0usize;
    for (i, (text, off)) in chunks.iter().enumerate() {
        if placeholder {
            cursor_row = i; // the cursor renders after the block
        } else if input.cursor >= *off && input.cursor <= off + text.chars().count() {
            cursor_row = i;
        }
    }
    let start = if total <= INPUT_MAX_ROWS {
        0
    } else {
        cursor_row
            .saturating_sub(INPUT_MAX_ROWS - 1)
            .min(total - INPUT_MAX_ROWS)
    };
    let end = (start + INPUT_MAX_ROWS).min(total);

    let mut rendered: Vec<Line<'static>> = Vec::new();
    for i in start..end {
        let (text, off) = &chunks[i];
        // No prompt prefix: the input bar text starts flush at the edge.
        if placeholder {
            let mut spans = vec![Span::styled(
                (*text).clone(),
                theme.input.placeholder.style(),
            )];
            if i == cursor_row {
                spans.push(Span::styled(" ", theme.input.cursor.style()));
            }
            rendered.push(Line::from(spans));
            continue;
        }
        if i == cursor_row {
            // Draw the cursor on its wrapped row.
            let cur = input.cursor.saturating_sub(*off);
            let before: String = text.chars().take(cur).collect();
            let at: String = text
                .chars()
                .nth(cur)
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".into());
            let after: String = text.chars().skip(cur + 1).collect();
            rendered.push(Line::from(vec![
                Span::styled(before, theme.input.text.style()),
                Span::styled(at, theme.input.cursor.style()),
                Span::styled(after, theme.input.text.style()),
            ]));
        } else {
            rendered.push(Line::from(Span::styled(
                (*text).clone(),
                theme.input.text.style(),
            )));
        }
    }
    let paragraph = Paragraph::new(Text::from(rendered)).style(theme.input.background.style());
    frame.render_widget(paragraph, inner);
    // Place the terminal cursor into the input bar for IME-friendly input.
    // x = display width of the wrapped row up to the cursor. CJK glyphs
    // occupy two cells, so use Unicode width, not char count.
    let col = if placeholder {
        UnicodeWidthStr::width(display.as_str())
    } else {
        let (text, off) = &chunks[cursor_row];
        let before: String = text
            .chars()
            .take(input.cursor.saturating_sub(*off))
            .collect();
        UnicodeWidthStr::width(before.as_str())
    } as u16;
    Some(Position::new(
        inner.x + col,
        inner.y + cursor_row.saturating_sub(start) as u16,
    ))
}

/// Scroll the transcript by a bounded number of visible rows.
pub fn scroll_lines(
    scroll: &mut ScrollState,
    area_height: usize,
    lines_total: usize,
    up: bool,
    rows: usize,
) {
    let rows = rows.max(1);
    if up {
        scroll.follow = false;
        scroll.offset = scroll.offset.saturating_sub(rows);
    } else {
        let max = lines_total.saturating_sub(area_height);
        let next = scroll.offset.saturating_add(rows).min(max);
        scroll.offset = next;
        if next >= max {
            scroll.follow = true;
        }
    }
}

/// One visible transcript page (used by PgUp/PgDn).
pub fn scroll_page(scroll: &mut ScrollState, area_height: usize, lines_total: usize, up: bool) {
    scroll_lines(
        scroll,
        area_height,
        lines_total,
        up,
        area_height.saturating_sub(1),
    );
}

/// Session picker state (design §3.6).
pub enum PickerAction<T> {
    None,
    Close,
    Select(T),
}

pub struct PickerState {
    pub sessions: Vec<crate::protocol::SessionInfo>,
    pub query: String,
    pub sel: usize,
}

impl Default for PickerState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            query: String::new(),
            sel: 0,
        }
    }
}

impl PickerState {
    pub fn handle_key(&mut self, key: &crossterm::event::KeyEvent) -> PickerAction<String> {
        match key.code {
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                PickerAction::Close
            }
            crossterm::event::KeyCode::Up => {
                self.sel = self.sel.saturating_sub(1);
                PickerAction::None
            }
            crossterm::event::KeyCode::Down => {
                let count = self.filtered().len();
                if count > 0 {
                    self.sel = (self.sel + 1).min(count - 1);
                }
                PickerAction::None
            }
            crossterm::event::KeyCode::Enter => self
                .filtered()
                .get(self.sel)
                .map(|index| PickerAction::Select(self.sessions[*index].id.clone()))
                .unwrap_or(PickerAction::None),
            crossterm::event::KeyCode::Char(character)
                if character == ' ' || !character.is_ascii_control() =>
            {
                self.query.push(character);
                self.sel = 0;
                PickerAction::None
            }
            crossterm::event::KeyCode::Backspace => {
                self.query.pop();
                self.sel = 0;
                PickerAction::None
            }
            _ => PickerAction::None,
        }
    }

    pub fn filtered(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                q.is_empty()
                    || s.title.to_lowercase().contains(&q)
                    || s.id.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// Full-screen session picker overlay (Ctrl+N).
pub fn render_picker(frame: &mut Frame, picker: &PickerState, theme: &Theme) {
    let area = frame.area();
    let width = (area.width * 70 / 100).clamp(40, area.width);
    let height = (area.height * 70 / 100).clamp(8, area.height);
    let rect = ratatui::layout::Rect {
        x: (area.width - width) / 2,
        y: (area.height - height) / 2,
        width,
        height,
    };
    let block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme.overlay.border.style())
        .title(" 会话选择 · Ctrl+N ")
        .style(theme.overlay.background.style());
    let inner = block.inner(rect);
    // Wipe the transcript cells underneath so short rows don't bleed.
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(block, rect);

    let filtered = picker.filtered();
    let mut rows: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled("> ", theme.overlay.accent.style()),
            Span::styled(picker.query.clone(), theme.overlay.text.style()),
            Span::styled("█", theme.overlay.accent.style()),
        ]),
        Line::from(Span::styled(
            "─".repeat(width.saturating_sub(2) as usize),
            theme.overlay.muted.style(),
        )),
    ];
    let list_height = inner.height.saturating_sub(4) as usize;
    let start = picker.sel.saturating_sub(list_height - 1);
    let window = filtered.iter().skip(start).take(list_height);
    for (i, &idx) in window.enumerate() {
        let s = &picker.sessions[idx];
        let marker = if i + start == picker.sel {
            "› "
        } else {
            "  "
        };
        let live = if s.live { "●" } else { " " };
        let title = if s.title.is_empty() {
            "(未命名会话)"
        } else {
            &s.title
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), theme.overlay.accent.style()),
            Span::styled(
                format!("{live} "),
                if s.live {
                    theme.overlay.selected_marker.style()
                } else {
                    theme.overlay.muted.style()
                },
            ),
            Span::styled(
                trim_to_width(title, width.saturating_sub(28) as usize),
                if i + start == picker.sel {
                    theme.overlay.selection.style()
                } else {
                    theme.overlay.text.style()
                },
            ),
        ];
        spans.push(Span::styled(
            format!("  {}", &s.id[..s.id.len().min(24)]),
            theme.overlay.muted.style(),
        ));
        rows.push(Line::from(spans));
    }
    if filtered.is_empty() {
        rows.push(Line::from(Span::styled(
            "（无匹配会话）",
            theme.overlay.muted.style(),
        )));
    }
    rows.push(Line::from(Span::styled(
        "↑↓ 选择   Enter 切换   Esc 退出",
        theme.overlay.muted.style(),
    )));
    frame.render_widget(Paragraph::new(Text::from(rows)), inner);
}

#[derive(Clone, Copy)]
struct InputPageRegions {
    header: ratatui::layout::Rect,
    body: ratatui::layout::Rect,
    footer: ratatui::layout::Rect,
}

fn input_page_shell(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    theme: &Theme,
) -> InputPageRegions {
    let block = Block::default()
        .style(Style::default().bg(theme.bg_soft))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);
    InputPageRegions {
        header: rows[0],
        body: rows[2],
        footer: rows[3],
    }
}

pub fn render_input_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    session: &mut InputPageSession,
    config: &crate::config::Config,
    theme: &Theme,
) {
    match &mut session.page {
        InputPage::Settings(settings) => render_settings(frame, area, settings, config, theme),
        InputPage::Login(login) => {
            render_login_scrolled(frame, area, login, &mut session.viewport, theme)
        }
        InputPage::Model(model) => render_model_page(
            frame,
            area,
            model,
            &session.focus,
            &mut session.viewport,
            theme,
        ),
        InputPage::Theme(page) => render_theme_page(
            frame,
            area,
            page,
            &session.focus,
            &mut session.viewport,
            theme,
        ),
    }
}

fn render_model_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &ModelPage,
    focus: &crate::input_page::FocusState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled("模型", Style::default().fg(theme.fg)),
        ])),
        regions.header,
    );
    if page.loading {
        frame.render_widget(
            Paragraph::new("读取模型目录中…").style(Style::default().fg(theme.dim)),
            regions.body,
        );
    } else {
        let columns = Layout::horizontal([
            Constraint::Percentage(38),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(regions.body);
        let visible = regions.body.height as usize;
        let provider_focus = focus.current.as_ref().and_then(|id| {
            id.0.strip_prefix("provider:")
                .and_then(|provider| page.providers.iter().position(|item| item.id == provider))
        });
        if let Some(index) = provider_focus {
            viewport.ensure_visible(index, visible, page.providers.len());
        }
        let mut providers = Vec::new();
        for provider in page.providers.iter().skip(viewport.start).take(visible) {
            let id = FocusId::new(format!("provider:{}", provider.id));
            let active = page.active_provider.as_deref() == Some(provider.id.as_str());
            let style = if focus.is(&id) {
                Style::default().fg(theme.fg).bg(theme.bg)
            } else {
                Style::default().fg(theme.fg)
            };
            providers.push(Line::from(vec![
                Span::styled(
                    if active { "● " } else { "○ " },
                    Style::default()
                        .fg(if active { theme.ok } else { theme.dim })
                        .bg(style.bg.unwrap_or(ratatui::style::Color::Reset)),
                ),
                Span::styled(
                    trim_to_width(&provider.name, columns[0].width.saturating_sub(2) as usize),
                    style,
                ),
            ]));
        }
        if page.providers.is_empty() {
            providers.push(Line::from(Span::styled(
                "（无可用提供商）",
                Style::default().fg(theme.dim),
            )));
        }
        frame.render_widget(Paragraph::new(providers), columns[0]);

        let active_provider = page.active_provider.as_deref().unwrap_or("");
        let models = page.active_models();
        let model_focus = focus.current.as_ref().and_then(|id| {
            id.0.strip_prefix(&format!("model:{active_provider}:"))
                .and_then(|model| models.iter().position(|item| item.id == model))
        });
        let model_start = model_focus
            .map(|index| index.saturating_sub(visible.saturating_sub(1)))
            .unwrap_or(0);
        let mut model_rows = Vec::new();
        for model in models.iter().skip(model_start).take(visible) {
            let id = FocusId::new(format!("model:{active_provider}:{}", model.id));
            let selected = page.current.as_ref().is_some_and(|(provider, current)| {
                provider == active_provider && current == &model.id
            });
            let style = if focus.is(&id) {
                Style::default().fg(theme.fg).bg(theme.bg)
            } else {
                Style::default().fg(theme.fg)
            };
            model_rows.push(Line::from(vec![
                Span::styled(
                    if selected { "● " } else { "○ " },
                    Style::default()
                        .fg(if selected { theme.ok } else { theme.dim })
                        .bg(style.bg.unwrap_or(ratatui::style::Color::Reset)),
                ),
                Span::styled(
                    trim_to_width(&model.name, columns[2].width.saturating_sub(2) as usize),
                    style,
                ),
            ]));
        }
        if models.is_empty() && !page.providers.is_empty() {
            model_rows.push(Line::from(Span::styled(
                "（该提供商无可用模型）",
                Style::default().fg(theme.dim),
            )));
        }
        frame.render_widget(Paragraph::new(model_rows), columns[2]);
    }
    frame.render_widget(
        Paragraph::new("hjkl/方向键移动   Enter 执行   Esc 退出")
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}

fn render_theme_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &ThemePage,
    focus: &crate::input_page::FocusState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled("主题", Style::default().fg(theme.fg)),
        ])),
        regions.header,
    );
    let focused = focus.current.as_ref().and_then(|id| {
        id.0.strip_prefix("theme:")
            .and_then(|name| page.themes.iter().position(|item| item.name == name))
    });
    if let Some(index) = focused {
        viewport.ensure_visible(index, regions.body.height as usize, page.themes.len());
    }
    let mut rows = Vec::new();
    for option in page
        .themes
        .iter()
        .skip(viewport.start)
        .take(regions.body.height as usize)
    {
        let id = FocusId::new(format!("theme:{}", option.name));
        let selected = option.name == page.current;
        let style = if focus.is(&id) {
            Style::default().fg(theme.fg).bg(theme.bg)
        } else {
            Style::default().fg(theme.fg)
        };
        let mut spans = vec![
            Span::styled(
                if selected { "● " } else { "○ " },
                Style::default()
                    .fg(if selected { theme.ok } else { theme.dim })
                    .bg(style.bg.unwrap_or(ratatui::style::Color::Reset)),
            ),
            Span::styled(format!("{:<18}", option.name), style),
        ];
        for color in [
            option.palette.bg,
            option.palette.fg,
            option.palette.user,
            option.palette.ok,
            option.palette.err,
            option.palette.running,
        ] {
            spans.push(Span::styled("  ", Style::default().bg(color)));
            spans.push(Span::raw(" "));
        }
        rows.push(Line::from(spans));
    }
    if rows.is_empty() {
        rows.push(Line::from(Span::styled(
            "（无可用主题）",
            Style::default().fg(theme.dim),
        )));
    }
    frame.render_widget(Paragraph::new(rows), regions.body);
    frame.render_widget(
        Paragraph::new("hjkl/方向键移动   Enter 应用   Esc 退出")
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}

/// /login Input Page. API-key edits render as bullets and confirmed actions
/// are sent to the bridge by the main-loop effect boundary.
pub fn render_login(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    theme: &Theme,
) {
    let mut viewport = crate::input_page::ViewportState::default();
    render_login_scrolled(frame, area, login, &mut viewport, theme);
}

fn render_login_scrolled(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);
    let buffer = frame.buffer_mut();
    let title = match &login.page {
        crate::login::Page::Menu => "登录",
        crate::login::Page::Providers => "API key · 选择提供商",
        crate::login::Page::ApiKey { .. } => "API key · 填写",
        crate::login::Page::Account => "Account · 网页登录",
        crate::login::Page::ProxyList => "Proxy · 已保存的代理",
        crate::login::Page::ProxyForm => "Proxy · 新建代理",
        crate::login::Page::ProxyDelete { .. } => "Proxy · 删除确认",
    };
    buffer.set_line(
        regions.header.x,
        regions.header.y,
        &Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user).bg(theme.bg_soft)),
            Span::styled(title, Style::default().fg(theme.fg).bg(theme.bg_soft)),
        ]),
        regions.header.width,
    );

    let item_area = regions.body;
    let area = item_area;
    let total_rows = match &login.page {
        crate::login::Page::Menu => 3,
        crate::login::Page::Providers => login.providers.len(),
        crate::login::Page::ProxyList => login.proxies.len() + 1,
        crate::login::Page::ProxyForm => crate::login::PROXY_ROWS,
        crate::login::Page::ProxyDelete { .. } => 3,
        _ => 1,
    };
    viewport.ensure_visible(login.pos, item_area.height as usize, total_rows);
    let start = viewport.start;
    match &login.page {
        crate::login::Page::Menu => {
            let items: &[(&str, &str)] = &[
                ("API key", "为各模型提供商填写 API key"),
                ("Account", "网页登录 Codex（ChatGPT 订阅）"),
                ("Proxy", "管理自定义代理端点"),
            ];
            for (i, (label, hint)) in items.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos,
                    false,
                    label,
                    Span::styled(
                        (*hint).to_string(),
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
        }
        crate::login::Page::Providers => {
            for (i, p) in login.providers.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                let value = if p.api_key_configured {
                    let hint = p.api_key_hint.as_deref().unwrap_or("");
                    Span::styled(
                        format!("已配置 {hint}"),
                        Style::default().fg(theme.ok).bg(theme.bg_soft),
                    )
                } else {
                    Span::styled(
                        "未配置（Enter 填写）",
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    )
                };
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos && p.api_key_writable,
                    false,
                    &p.name,
                    value,
                    theme,
                );
            }
            if login.providers.is_empty() && !login.loading {
                buffer.set_line(
                    area.x,
                    item_area.y,
                    &Line::from(Span::styled(
                        "没有可用的提供商",
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    )),
                    item_area.width,
                );
            }
        }
        crate::login::Page::ApiKey { .. } => {
            let buf = login.editing.as_deref().unwrap_or("");
            let shown = format!("{}█", "●".repeat(buf.chars().count()));
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(
                    shown,
                    Style::default().fg(theme.ok).bg(theme.bg),
                )),
                item_area.width,
            );
        }
        crate::login::Page::Account => {
            let (status, url) = if login.codex_pending {
                (
                    "请在网页登录：",
                    login.codex_verification_uri.as_deref().unwrap_or(""),
                )
            } else if login.codex_logged_in() {
                let id = login
                    .codex
                    .as_ref()
                    .and_then(|c| c.account_id.as_deref())
                    .unwrap_or("");
                ("已登录 Codex", id)
            } else {
                ("按 Enter 开始网页登录", "")
            };
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(
                    status,
                    Style::default().fg(theme.fg).bg(theme.bg_soft),
                )),
                item_area.width,
            );
            if !url.is_empty() {
                buffer.set_line(
                    area.x,
                    item_area.y + 1,
                    &Line::from(Span::styled(
                        url,
                        Style::default().fg(theme.user).bg(theme.bg_soft),
                    )),
                    item_area.width,
                );
                if let Some(code) = login.codex_user_code.as_deref() {
                    buffer.set_line(
                        area.x,
                        item_area.y + 2,
                        &Line::from(Span::styled(
                            format!("用户代码：{code}"),
                            Style::default().fg(theme.ok).bg(theme.bg_soft),
                        )),
                        item_area.width,
                    );
                }
            }
        }
        crate::login::Page::ProxyList => {
            for (i, p) in login.proxies.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos,
                    false,
                    &p.name,
                    Span::styled(
                        p.base_url.clone(),
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
            let new_index = login.proxies.len();
            let new_y = item_area.y + new_index.saturating_sub(start) as u16;
            if new_index >= start && new_y < item_area.y + item_area.height {
                login_list_row(
                    buffer,
                    area,
                    new_y,
                    login.pos == login.proxies.len(),
                    false,
                    "+ New",
                    Span::styled(
                        "新建代理".to_string(),
                        Style::default().fg(theme.user).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
        }
        crate::login::Page::ProxyForm => {
            let fields: &[&str] = &["base url", "api key", "协议模式", "模型名称"];
            for (i, label) in fields.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                let editing = i == login.pos && login.editing.is_some();
                let value = if editing {
                    let buf = login.editing.as_deref().unwrap_or("");
                    let shown = if i == 1 {
                        "●".repeat(buf.chars().count())
                    } else {
                        buf.to_string()
                    };
                    Span::styled(
                        format!("{shown}█"),
                        Style::default().fg(theme.ok).bg(theme.bg),
                    )
                } else {
                    let v = match i {
                        2 => crate::login::PROTOCOLS
                            [login.draft.protocol % crate::login::PROTOCOLS.len()]
                        .1
                        .to_string(),
                        _ => match i {
                            0 => login.draft.base_url.clone(),
                            1 => login.draft.api_key.clone(),
                            _ => login.draft.model.clone(),
                        },
                    };
                    let shown = if i == 2 {
                        format!("◄ {v} ►")
                    } else if v.is_empty() {
                        "（可选）".to_string()
                    } else if i == 1 {
                        "●".repeat(v.chars().count())
                    } else {
                        v
                    };
                    Span::styled(shown, Style::default().fg(theme.dim).bg(theme.bg_soft))
                };
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos,
                    editing,
                    label,
                    value,
                    theme,
                );
            }
            let save_index = crate::login::PROXY_SAVE_ROW;
            let save_y = item_area.y + save_index.saturating_sub(start) as u16;
            if save_index >= start && save_y < item_area.y + item_area.height {
                login_list_row(
                    buffer,
                    area,
                    save_y,
                    login.pos == crate::login::PROXY_SAVE_ROW,
                    false,
                    "保存",
                    Span::styled(
                        "保存并创建".to_string(),
                        Style::default().fg(theme.user).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
        }
        crate::login::Page::ProxyDelete { name, .. } => {
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(
                    format!("确定删除代理“{name}”？"),
                    Style::default().fg(theme.fg).bg(theme.bg_soft),
                )),
                item_area.width,
            );
            if item_area.height > 1 {
                login_list_row(
                    buffer,
                    area,
                    item_area.y + 1,
                    login.pos == 0,
                    false,
                    "取消",
                    Span::styled(
                        "保留该代理",
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
            if item_area.height > 2 {
                login_list_row(
                    buffer,
                    area,
                    item_area.y + 2,
                    login.pos == 1,
                    false,
                    "删除",
                    Span::styled("永久删除", Style::default().fg(theme.err).bg(theme.bg_soft)),
                    theme,
                );
            }
        }
    }

    // Footer: the last rejected write outranks the hint.
    let footer = if let Some(error) = login.error.as_deref() {
        format!("✗ {error}")
    } else if login.loading {
        "读取中…".to_string()
    } else {
        match &login.page {
            crate::login::Page::Menu => "↑/↓ 选择   Enter 进入   Esc 退出".to_string(),
            crate::login::Page::Providers => "↑/↓ 选择   Enter 填写   Esc 返回".to_string(),
            crate::login::Page::ApiKey { .. } => "Enter 保存   Esc 返回 · 密钥不会回显".to_string(),
            crate::login::Page::Account => "Enter 开始登录   Esc 返回".to_string(),
            crate::login::Page::ProxyList => "↑/↓ 选择   Enter 执行   Esc 返回".to_string(),
            crate::login::Page::ProxyForm => {
                "↑/↓ 选字段   Enter 编辑/切换   Enter 保存   Esc 返回".to_string()
            }
            crate::login::Page::ProxyDelete { .. } => {
                "←/→ 选择   Enter 确认   Esc 取消".to_string()
            }
        }
    };
    let fg = if login.error.is_some() {
        theme.err
    } else {
        theme.dim
    };
    buffer.set_line(
        regions.footer.x,
        regions.footer.y,
        &Line::from(Span::styled(
            footer,
            Style::default().fg(fg).bg(theme.bg_soft),
        )),
        regions.footer.width,
    );
}

/// Render one login list row: a fixed-width label column followed by a value.
fn login_list_row(
    buffer: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    y: u16,
    focused: bool,
    editing: bool,
    label: &str,
    value: Span<'static>,
    theme: &Theme,
) {
    let name_bg = if focused && !editing {
        theme.bg
    } else {
        theme.bg_soft
    };
    let label_width = UnicodeWidthStr::width(label);
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        label.to_string(),
        Style::default().fg(theme.fg).bg(name_bg),
    )];
    if label_width < 12 {
        spans.push(Span::styled(
            " ".repeat(12 - label_width),
            Style::default().fg(theme.fg).bg(name_bg),
        ));
    }
    spans.push(value);
    let line = Line::from(spans);
    let line_width = (line.width() as u16).min(area.width);
    buffer.set_line(area.x, y, &line, line_width);
}

/// /settings overlay (design §4.7): category column + item list, live edit.
pub fn render_settings(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    settings: &mut SettingsState,
    config: &crate::config::Config,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);

    // ---- category tabs: actionable members of the single focus graph ----
    {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, cat) in crate::settings::CATEGORIES.iter().enumerate() {
            let fg = if i == settings.category {
                theme.ok
            } else {
                theme.fg
            };
            let bg = if settings.focus_tabs && i == settings.tab_cursor {
                theme.bg
            } else {
                theme.bg_soft
            };
            spans.push(Span::styled(
                format!(" {cat} "),
                Style::default().fg(fg).bg(bg),
            ));
            spans.push(Span::styled(
                "  ",
                Style::default().fg(theme.fg).bg(theme.bg_soft),
            ));
        }
        let line = Line::from(spans);
        let width = (line.width() as u16).min(regions.header.width);
        let x = regions.header.x + regions.header.width.saturating_sub(width) / 2;
        let buffer = frame.buffer_mut();
        buffer.set_line(x, regions.header.y, &line, width);
    }

    // ---- two columns with a 2-space gap: small name/description column
    // ---- left, values right.
    let cols = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Length(2),
        Constraint::Min(1),
    ])
    .split(regions.body);
    let left_width = cols[0].width as usize;
    let value_width = cols[2].width;
    let items = crate::settings::items_in(settings.category);
    let focus_idx = (!settings.focus_tabs).then_some(settings.pos[settings.category]);
    let mut left_rows: Vec<Line<'static>> = Vec::new();
    let mut right_rows: Vec<Line<'static>> = Vec::new();
    // (item index, first row, row count) for scroll anchoring.
    let mut anchors: Vec<(usize, usize, usize)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let focused = focus_idx == Some(idx);
        // While editing, the focus moves to the value: the name loses the
        // Night highlight.
        let editing = focused && settings.editing.is_some();
        let name_bg = if focused && !editing {
            theme.bg
        } else {
            theme.bg_soft
        };
        let first = left_rows.len();
        // Name row: fg text; the focused NAME fills with Night (the
        // description below is never selected).
        let mut name_line = Line::from(Span::styled(
            (*item).label,
            Style::default().fg(theme.fg).bg(name_bg),
        ));
        if focused && !editing {
            let used = name_line.width();
            if used < left_width {
                name_line.push_span(Span::styled(
                    " ".repeat(left_width - used),
                    Style::default().fg(theme.fg).bg(theme.bg),
                ));
            }
        }
        left_rows.push(name_line);
        // Description: Bark foreground on Ash, wrapped under the name.
        let mut height = 1usize;
        for chunk in wrap_text(item.desc, left_width.max(1)) {
            left_rows.push(Line::from(Span::styled(
                chunk,
                Style::default().fg(theme.dim).bg(theme.bg_soft),
            )));
            height += 1;
        }
        right_rows.push(settings_value_line(
            item,
            config,
            theme,
            settings,
            focused,
            value_width,
        ));
        for _ in 1..height {
            right_rows.push(Line::default());
        }
        anchors.push((idx, first, height));
    }
    // Keep the focused item visible (display-only scroll).
    let visible = regions.body.height as usize;
    if let Some((_, first, height)) = anchors
        .iter()
        .find(|(idx, _, _)| *idx == settings.pos[settings.category])
    {
        if settings.scroll > *first {
            settings.scroll = *first;
        }
        if first + height > settings.scroll + visible {
            settings.scroll = (first + height).saturating_sub(visible);
        }
    }
    settings.scroll = settings.scroll.min(left_rows.len().saturating_sub(1));
    let end = (settings.scroll + visible).min(left_rows.len());
    let left_slice: Vec<Line<'static>> = left_rows[settings.scroll..end].to_vec();
    let right_slice: Vec<Line<'static>> = right_rows[settings.scroll..end].to_vec();
    frame.render_widget(Paragraph::new(Text::from(left_slice)), cols[0]);
    frame.render_widget(Paragraph::new(Text::from(right_slice)), cols[2]);

    // ---- key hints ----
    let hint = if settings.editing.is_some() {
        "Enter 确认   Esc 取消修改"
    } else {
        "hjkl/方向键移动   Enter 执行   Esc 退出 · 即改即存"
    };
    let buffer = frame.buffer_mut();
    buffer.set_line(
        regions.footer.x,
        regions.footer.y,
        &Line::from(Span::styled(
            hint,
            Style::default().fg(theme.dim).bg(theme.bg_soft),
        )),
        regions.footer.width,
    );
}

/// The value cell of one settings row. Choice values show every option as
/// `○ label` (unselected, default fg) / `● label` (selected, green). The
/// cell sits on Ash; while editing, the focused element (cursor option or
/// input buffer) turns Night.
fn settings_value_line(
    item: &crate::settings::ItemDef,
    config: &crate::config::Config,
    theme: &Theme,
    settings: &SettingsState,
    focused_row: bool,
    width: u16,
) -> Line<'static> {
    let editing_input =
        focused_row && matches!(settings.editing, Some(crate::settings::Edit::Input { .. }));
    let editing_choice = match settings.editing {
        Some(crate::settings::Edit::Choice { cursor }) if focused_row => Some(cursor),
        _ => None,
    };
    let cell_bg = if editing_input {
        theme.bg
    } else {
        theme.bg_soft
    };
    let line: Line<'static> = match item.kind {
        crate::settings::ItemKind::Choice { options } => {
            let current = (item.get)(config);
            let selected = options
                .iter()
                .position(|o| (*o).starts_with(current.as_str()));
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, opt) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(
                        "  ",
                        Style::default().fg(theme.fg).bg(cell_bg),
                    ));
                }
                let is_sel = Some(i) == selected;
                let (glyph, fg) = if is_sel {
                    ("●", theme.ok)
                } else {
                    ("○", theme.fg)
                };
                let bg = if Some(i) == editing_choice {
                    theme.bg
                } else {
                    cell_bg
                };
                spans.push(Span::styled(
                    format!("{glyph} {opt}"),
                    Style::default().fg(fg).bg(bg),
                ));
            }
            Line::from(spans)
        }
        crate::settings::ItemKind::ModeChoice | crate::settings::ItemKind::ThemeChoice => {
            // Same ○/● rendering over the live roster (the current value is
            // appended when the roster no longer lists it).
            let options =
                crate::settings::dynamic_options(item, config, &settings.modes, &settings.themes);
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, opt) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(
                        "  ",
                        Style::default().fg(theme.fg).bg(cell_bg),
                    ));
                }
                let is_sel = *opt == (item.get)(config);
                let (glyph, fg) = if is_sel {
                    ("●", theme.ok)
                } else {
                    ("○", theme.fg)
                };
                let bg = if Some(i) == editing_choice {
                    theme.bg
                } else {
                    cell_bg
                };
                spans.push(Span::styled(
                    format!("{glyph} {opt}"),
                    Style::default().fg(fg).bg(bg),
                ));
            }
            Line::from(spans)
        }
        crate::settings::ItemKind::Input => {
            let text = if editing_input {
                match &settings.editing {
                    Some(crate::settings::Edit::Input { buf }) => format!("{buf}█"),
                    _ => String::new(),
                }
            } else {
                (item.get)(config)
            };
            let fg = if editing_input { theme.ok } else { theme.fg };
            let mut input_line =
                Line::from(Span::styled(text, Style::default().fg(fg).bg(cell_bg)));
            if editing_input {
                // Fill the row so the focused input cell reads as a Night block.
                let used = input_line.width();
                if used < width as usize {
                    input_line.push_span(Span::styled(
                        " ".repeat(width as usize - used),
                        Style::default().fg(theme.fg).bg(theme.bg),
                    ));
                }
            }
            input_line
        }
        crate::settings::ItemKind::ReadOnly => Line::from(Span::styled(
            trim_to_width(&(item.get)(config), width as usize),
            Style::default().fg(theme.dim).bg(cell_bg),
        )),
    };
    line
}

/// Char-level wrap of `text` at `width` display columns (descriptions mix
/// CJK and ASCII; spaces are not treated as word boundaries). Shared with
/// copy-mode row math.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + w > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push(ch);
        used += w;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Trim `text` to at most `width` display columns, appending `…` when it
/// overflows (the ellipsis itself counts against the budget).
fn trim_to_width(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + w + 1 > width {
            out.push('…');
            break;
        }
        used += w;
        out.push(ch);
    }
    out
}

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
            styled_msg_lines(&s.msgs[0], &s, 80).is_empty(),
            "reasoning block renders nothing"
        );
        let card_lines = styled_msg_lines(&s.msgs[1], &s, 20);
        assert!(card_lines.len() >= 4);
        assert!(card_lines.iter().all(|line| line.width() == 20));
        let rows = copy_layout_rows(&s);
        // Hidden reasoning is not copyable; the visible card still is.
        assert!(!rows.iter().any(|row| row.unit == 10));
        assert!(rows.iter().any(|row| row.unit == 11));
    }

    #[test]
    fn user_block_has_vertical_padding() {
        let s = state_with(vec![Msg::User {
            text: "你好".into(),
        }]);
        let lines = styled_msg_lines(&s.msgs[0], &s, 80);
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
        let lines = styled_msg_lines(&s.msgs[0], &s, 80);
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
        let lines = styled_msg_lines(&s.msgs[0], &s, 80);
        assert_eq!(lines.len(), 5);
    }

    /// Wrapped continuation rows keep the configured horizontal gutter.
    #[test]
    fn user_block_wrapped_rows_keep_gutter() {
        let s = state_with(vec![Msg::User {
            text: "x".repeat(30),
        }]);
        // Width 12, gutter 2 → three wrapped rows of 10 chars each.
        let lines = styled_msg_lines(&s.msgs[0], &s, 12);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
        let running: String = msg_lines(&s.msgs[0], &s)[0]
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
        let done: String = msg_lines(&s.msgs[0], &s)[0]
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
        let lines = msg_lines(&s.msgs[0], &s);
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
        let lines = msg_lines(&s2.msgs[0], &s2);
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
        let lines = msg_lines(&s3.msgs[0], &s3);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
            codex: None,
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
        // Menu page: the three choices render.
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
        assert!(all.contains("Account"), "Account menu item");
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
            modes: false,
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
        let lines = msg_lines(s.msgs.last().unwrap(), &s);
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
        let lines = msg_lines(s.msgs.last().unwrap(), &s);
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
        let lines = styled_msg_lines(&s.msgs[0], &s, 80);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
        let lines = msg_lines(&s.msgs[0], &s);
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
            state.msgs.push(Msg::System {
                text: format!("settled {index}"),
            });
        }
        state.start_thinking();
        let active_index = state.msgs.len() - 1;
        state.msgs.push(Msg::Block(TranscriptBlock {
            id: DisplayId::event(9_999, "reasoning"),
            unit: None,
            content: "hidden stream".into(),
            format: TranscriptFormat::Reasoning,
            tone: DisplayTone::Dim,
            copy_source: "hidden stream".into(),
            streaming: true,
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
        state.msgs[0] = Msg::User {
            text: "x".repeat(80),
        };
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
