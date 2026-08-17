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
    config::Theme,
    input::{InputState, Suggestion},
    login::LoginState,
    model::{breathing_color, settle_color, AgentStatus, AppState, Msg, ToolState},
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
        Self { follow: true, offset: 0 }
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

/// Per-frame overlay/panel handles handed to `render` — one struct instead
/// of five trailing parameters (the panels are owned by the main loop, the
/// renderer only borrows them for one frame).
pub struct RenderOverlays<'a> {
    pub help_visible: bool,
    pub overlay: Option<&'a CopyOverlay>,
    pub toast: Option<&'a str>,
    pub settings: Option<&'a mut SettingsState>,
    pub login: Option<&'a mut LoginState>,
}

pub fn render(
    frame: &mut Frame,
    state: &mut AppState,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: RenderOverlays<'_>,
) {
    let RenderOverlays {
        help_visible,
        overlay,
        toast,
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
    let input_rows = input_rows(input);
    let approval_rows = if state.approval.is_some() { 3 } else { 0 };
    let question_rows = if state.question.is_some() { 3 } else { 0 };
    let settings_open = settings.is_some();
    let login_open = login.is_some();
    // The settings / login panels replace the input bar and take two thirds
    // of the page height (no floating window); otherwise the bottom chunk is
    // the ordinary input bar. The transcript keeps the top third.
    let settings_rows = ((area.height as u32) * 2 / 3)
        .min(area.height.saturating_sub(3) as u32) as u16;
    let bottom_rows = if settings_open || login_open {
        settings_rows
    } else {
        (input_rows + 2) as u16
    };
    let bottom = Constraint::Length(bottom_rows);
    // Pending-prompt queue: one row per queued item above the input bar
    // (bounded by the remaining height; an overflow marker summarizes).
    let max_queue = area
        .height
        .saturating_sub(1 + question_rows + approval_rows + bottom_rows + 3) as usize;
    let queue_visible = state.queue.len().min(max_queue);
    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(question_rows),
        Constraint::Length(approval_rows),
        Constraint::Length(queue_visible as u16),
        bottom,
        Constraint::Length(1), // one-row gap between input and status bar
        Constraint::Length(1), // status bar
        Constraint::Length(1), // session title row at the very bottom
    ])
    .split(page);

    render_transcript(frame, chunks[0], state, scroll, theme, help_visible, overlay);
    if let Some(question) = state.question.as_ref() {
        render_question(frame, chunks[1], question, theme);
    }
    if approval_rows > 0 {
        render_approval(frame, chunks[2], state.approval.as_ref().unwrap(), theme);
    }
    if queue_visible > 0 {
        render_queue(frame, chunks[3], &state.queue, queue_visible, theme);
    }
    if let Some(settings) = settings.as_mut() {
        // The input bar is replaced by the settings panel.
        render_settings(frame, chunks[4], settings, &state.config, theme);
    } else if let Some(login) = login.as_mut() {
        // The input bar becomes the login settings page (/login).
        render_login(frame, chunks[4], login, theme);
    } else if let Some(question) = state.question.as_ref() {
        // The input bar becomes the selection bar while a question pends.
        render_question_bar(
            frame,
            chunks[4],
            question,
            theme,
            state.config.user_input_padding as u16,
        );
    } else {
        render_input(
            frame,
            chunks[4],
            input,
            theme,
            overlay.is_some(),
            toast,
            state.config.user_input_padding as u16,
        );
    }
    render_status(frame, chunks[6], state, scroll, theme);
    render_title(frame, chunks[7], state, theme);
    // Slash-command suggestions float above the input bar (last draw wins).
    if !settings_open && !login_open && state.question.is_none() {
        if let Some(suggest) = input.suggest.as_ref() {
            render_suggest(frame, suggest, chunks[4], theme);
        }
    }
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
    let rows = suggest.matches.len() as u16;
    let width = 46u16.min(input_area.width);
    let rect = ratatui::layout::Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(rows + 2),
        width,
        height: rows + 2,
    };
    frame.render_widget(ratatui::widgets::Clear, rect);
    let panel = Style::default().bg(theme.bg_soft);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("❯ ", Style::default().fg(theme.user)),
        Span::styled(
            if suggest.modes { "模式" } else { "命令" },
            Style::default().fg(theme.dim),
        ),
    ]));
    for (i, cmd) in suggest.matches.iter().enumerate() {
        let selected = i == suggest.sel;
        let desc = suggest.descriptions.get(i).map(String::as_str).unwrap_or("");
        let row_style = if selected {
            Style::default().fg(theme.bg).bg(theme.fg)
        } else {
            Style::default().fg(theme.fg)
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "❯ " } else { "  " },
                Style::default().fg(theme.user),
            ),
            Span::styled(cmd.clone(), row_style),
            Span::styled(
                format!("  {desc}"),
                if selected {
                    row_style
                } else {
                    Style::default().fg(theme.dim)
                },
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "↑↓ 选择 · Enter 发送 · Esc 关闭",
        Style::default().fg(theme.dim),
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
        Span::styled("⚠ 审批", Style::default().fg(theme.running).add_modifier(Modifier::BOLD)),
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
        Span::styled("❓ ", Style::default().fg(theme.user).add_modifier(Modifier::BOLD)),
        Span::styled(header, Style::default().fg(theme.user).add_modifier(Modifier::BOLD)),
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
        batch.current_options()
            .get(batch.sel)
            .and_then(|o| o.description.clone())
            .unwrap_or_else(|| "←→ 切换选项 · Enter 选中".to_string())
    };
    rows.push(Line::from(Span::styled(detail, Style::default().fg(theme.dim))));
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
) {
    let block = Block::default()
        .style(Style::default().bg(theme.bg_soft))
        .padding(Padding::new(padding, padding, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let hint = format!(
        "{}/{} Enter {} · Esc 取消",
        batch.current + 1,
        batch.questions.len(),
        if batch.current + 1 < batch.questions.len() { "下一项" } else { "确定" }
    );
    let options = batch.current_options();
    let left: Line<'static>;
    if options.is_empty() {
        left = Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled(batch.draft.clone(), Style::default().fg(theme.fg)),
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
                spans.push(Span::styled(option.label.clone(), Style::default().fg(theme.fg)));
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
        // Keep the terminal cursor on the draft for IME-friendly input.
        let col = unicode_width::UnicodeWidthStr::width(batch.draft.as_str()) as u16;
        frame.set_cursor_position(Position::new(inner.x + 2 + col, inner.y));
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
    scroll: &ScrollState,
    theme: &Theme,
) {
    let dim = Style::default().fg(theme.dim);
    // Running bullet leads the status bar: yellow breathing while the agent
    // is running (or has just been sent work), gray while idle. One space
    // separates it from the elements that follow.
    let bullet = if state.status == AgentStatus::Running || state.working {
        Span::styled("•", Style::default().fg(breathing_color(theme, state.breath_phase())))
    } else {
        Span::styled("•", dim)
    };
    let mut spans = vec![
        bullet,
        Span::styled(" ", dim),
        Span::styled("e", dim),
        Span::styled(" · ", dim),
    ];
    if state.config.show_model_in_status {
        if let (Some(provider), Some(model)) = (&state.provider, &state.model) {
            spans.push(Span::styled(format!("{provider} · {model}"), dim));
            spans.push(Span::styled(" · ", dim));
        }
    }
    if state.question.is_some() {
        spans.push(Span::styled("⏳ 等待选择", Style::default().fg(theme.running)));
        spans.push(Span::styled(" · ", dim));
    }
    let left = Line::from(spans);
    let mut right_spans = Vec::new();
    if !scroll.follow {
        right_spans.push(Span::styled("↓ 新消息 ", dim));
    }
    right_spans.push(Span::styled("[Ctrl+B 复制] [Ctrl+H 帮助]", dim));
    let right = Line::from(right_spans).alignment(ratatui::layout::Alignment::Right);
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
    let style = Style::default().fg(theme.dim).bg(theme.bg);
    let buffer = frame.buffer_mut();
    let width = area.width as usize;

    // Left-aligned title, truncated to leave the path (plus a small gap)
    // visible. Drawn first so the path below wins any overlap (defensive:
    // the truncation already reserves the path's columns).
    let title = match state.session_title.as_deref() {
        Some(title) if !title.trim().is_empty() => format!("❯ {}", title.trim()),
        _ => String::new(),
    };
    let cwd = state.session_cwd.as_deref().unwrap_or("").trim().to_string();
    let path_w = UnicodeWidthStr::width(cwd.as_str());
    if !title.is_empty() {
        let avail = width.saturating_sub(path_w.saturating_add(2));
        let shown = if UnicodeWidthStr::width(title.as_str()) > avail {
            trim_to_width(&title, avail)
        } else {
            title
        };
        let left = Line::from(Span::styled(shown, style));
        buffer.set_line(area.x, area.y, &left, area.width);
    }

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
        .map(|(f, n)| if *n > 1 { format!("{f} x{n}") } else { f.clone() })
        .collect::<Vec<_>>()
        .join(", ")
}

fn msg_lines(msg: &Msg, state: &AppState) -> Vec<Line<'static>> {
    let theme = state.theme();
    match msg {
        // User blocks are built width-aware (wrapped with the gutter on
        // every row) in styled_msg_lines.
        Msg::User { .. } => Vec::new(),
        Msg::Assistant { lines, .. } => lines.iter().map(|r| r.line.clone()).collect(),
        Msg::Streaming { text } => {
            let mut lines = assistant_lines(text, true, &theme, 0, 0);
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
            let (glyph, color, tail) = match &card.state {
                ToolState::Running => (
                    "•".to_string(),
                    breathing_color(&theme, state.breath_phase()),
                    String::new(),
                ),
                ToolState::Done { ok: true, lines, duration_ms } => {
                    let dur = if state.config.show_tool_duration {
                        format!(" · {:.1}s", *duration_ms as f64 / 1000.0)
                    } else {
                        String::new()
                    };
                    // Done bullet: transition from the captured breathing
                    // color to green instead of snapping.
                    let color = match (&card.done_since, &card.done_from) {
                        (Some(since), Some(from)) => settle_color(*from, theme.ok, since.elapsed()),
                        _ => theme.ok,
                    };
                    ("•".to_string(), color, format!(" · {lines} 行{dur}"))
                }
                ToolState::Done { ok: false, lines, duration_ms } => {
                    let dur = if state.config.show_tool_duration {
                        format!(" · {:.1}s", *duration_ms as f64 / 1000.0)
                    } else {
                        String::new()
                    };
                    let color = match (&card.done_since, &card.done_from) {
                        (Some(since), Some(from)) => settle_color(*from, theme.err, since.elapsed()),
                        _ => theme.err,
                    };
                    ("•".to_string(), color, format!(" · {lines} 行{dur}"))
                }
            };
            vec![Line::from(vec![
                Span::styled("  ", Style::default().fg(theme.dim)),
                Span::styled(glyph, Style::default().fg(color)),
                Span::styled(" ", Style::default().fg(theme.dim)),
                // Tool name in umber (#4d424b, the `selection` slot).
                Span::styled(card.name.clone(), Style::default().fg(theme.selection)),
                Span::styled(" ", Style::default().fg(theme.dim)),
                // Summary text in bark (#6f5d63, the `dim` slot).
                Span::styled(card.summary.clone(), Style::default().fg(theme.dim)),
                Span::styled(tail, Style::default().fg(theme.dim)),
            ])]
        }
        Msg::Thinking(card) => {
            // Same shape as a tool card: breathing bullet while the model
            // thinks, settling to green when the phase completes. Consecutive
            // phases collapse into one row with a count.
            let color = match card.state {
                crate::model::ThinkState::Running => {
                    breathing_color(&theme, state.breath_phase())
                }
                crate::model::ThinkState::Done => match (&card.done_since, &card.done_from) {
                    (Some(since), Some(from)) => settle_color(*from, theme.ok, since.elapsed()),
                    _ => theme.ok,
                },
            };
            let mut spans = vec![
                Span::styled("  ", Style::default().fg(theme.dim)),
                Span::styled("•", Style::default().fg(color)),
                Span::styled(" ", Style::default().fg(theme.dim)),
                Span::styled("Thinking...", Style::default().fg(theme.selection)),
            ];
            if card.count > 1 {
                spans.push(Span::styled(
                    format!(" x{}", card.count),
                    Style::default().fg(theme.dim),
                ));
            }
            vec![Line::from(spans)]
        }
        Msg::FileGroup(group) => {
            // Text runs: `read`/`edit` labels in umber, file lists in bark.
            // Repeated reads/edits of the same file collapse into `name xN`.
            // Successful items fold onto one green-bullet line; failed items
            // are listed separately (red bullet, one line per file) and never
            // join the fold.
            let dim = Style::default().fg(theme.dim);
            let umber = Style::default().fg(theme.selection);
            let err = Style::default().fg(theme.err);
            let ok_style = Style::default().fg(theme.ok);
            let breath = Style::default().fg(breathing_color(&theme, state.breath_phase()));
            // Full paths for dedupe; the display shows basenames + counts.
            let pending_reads: Vec<String> = group
                .reads
                .iter()
                .filter(|i| i.ok.is_none())
                .map(|i| i.file.clone())
                .collect();
            let failed_reads: Vec<String> = group
                .reads
                .iter()
                .filter(|i| i.ok == Some(false))
                .map(|i| i.file.clone())
                .collect();
            let ok_reads: Vec<String> = group
                .reads
                .iter()
                .filter(|i| i.ok == Some(true))
                .map(|i| i.file.clone())
                .collect();
            let pending_edits: Vec<String> = group
                .edits
                .iter()
                .filter(|i| i.ok.is_none())
                .map(|i| i.file.clone())
                .collect();
            let failed_edits: Vec<String> = group
                .edits
                .iter()
                .filter(|i| i.ok == Some(false))
                .map(|i| i.file.clone())
                .collect();
            let ok_edits: Vec<String> = group
                .edits
                .iter()
                .filter(|i| i.ok == Some(true))
                .map(|i| i.file.clone())
                .collect();
            let fail_line = |label: &'static str, file: &str| -> Line<'static> {
                Line::from(vec![
                    Span::styled("  ", dim),
                    Span::styled("•", err),
                    Span::styled(" ", dim),
                    Span::styled(label, umber),
                    Span::styled(" ", dim),
                    Span::styled(file.to_string(), dim),
                ])
            };
            // One red line per DISTINCT failed file; repeats fold into xN.
            let fail_lines = |label: &'static str, files: &[String], out: &mut Vec<Line<'static>>| {
                for (name, n) in counted_files(files) {
                    let text = if n > 1 { format!("{name} x{n}") } else { name };
                    out.push(fail_line(label, &text));
                }
            };

            let mut lines = Vec::new();
            if !pending_reads.is_empty() {
                // • read a x2, b (bullet breathes while reading)
                lines.push(Line::from(vec![
                    Span::styled("  ", dim),
                    Span::styled("•", breath),
                    Span::styled(" ", dim),
                    Span::styled("read", umber),
                    Span::styled(" ", dim),
                    Span::styled(file_list(&counted_files(&pending_reads)), dim),
                ]));
                fail_lines("read", &failed_reads, &mut lines);
            } else if !pending_edits.is_empty() {
                // • read a, b;
                // • edit c (bullet breathes while editing)
                if !ok_reads.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("  ", dim),
                        Span::styled("•", ok_style),
                        Span::styled(" ", dim),
                        Span::styled("read", umber),
                        Span::styled(" ", dim),
                        Span::styled(file_list(&counted_files(&ok_reads)), dim),
                        Span::styled(";", dim),
                    ]));
                }
                fail_lines("read", &failed_reads, &mut lines);
                lines.push(Line::from(vec![
                    Span::styled("  ", dim),
                    Span::styled("•", breath),
                    Span::styled(" ", dim),
                    Span::styled("edit", umber),
                    Span::styled(" ", dim),
                    Span::styled(file_list(&counted_files(&pending_edits)), dim),
                ]));
                fail_lines("edit", &failed_edits, &mut lines);
            } else {
                // Folded line: successful reads + edits, bullet interpolates
                // from the captured breathing color to green on settle.
                if !ok_reads.is_empty() || !ok_edits.is_empty() {
                    let bullet_color = match (&group.done_since, &group.done_from) {
                        (Some(since), Some(from)) => settle_color(*from, theme.ok, since.elapsed()),
                        _ => theme.ok,
                    };
                    let mut spans = vec![
                        Span::styled("  ", dim),
                        Span::styled("•", Style::default().fg(bullet_color)),
                        Span::styled(" ", dim),
                    ];
                    if !ok_reads.is_empty() {
                        spans.push(Span::styled("read", umber));
                        spans.push(Span::styled(" ", dim));
                        spans.push(Span::styled(file_list(&counted_files(&ok_reads)), dim));
                    }
                    if !ok_edits.is_empty() {
                        if !ok_reads.is_empty() {
                            spans.push(Span::styled("; ", dim));
                        }
                        spans.push(Span::styled("edit", umber));
                        spans.push(Span::styled(" ", dim));
                        spans.push(Span::styled(file_list(&counted_files(&ok_edits)), dim));
                    }
                    lines.push(Line::from(spans));
                }
                fail_lines("read", &failed_reads, &mut lines);
                fail_lines("edit", &failed_edits, &mut lines);
            }
            lines
        }
        Msg::System { text } => vec![Line::from(vec![Span::styled(
            text.clone(),
            Style::default().fg(theme.dim),
        )])],
        Msg::Error { text } => vec![Line::from(vec![
            Span::styled("✗ ", Style::default().fg(theme.err)),
            Span::styled(text.clone(), Style::default().fg(theme.err)),
        ])],
    }
}

fn assistant_lines(
    text: &str,
    _streaming: bool,
    theme: &Theme,
    _start: usize,
    _end: usize,
) -> Vec<Line<'static>> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .into_iter()
        .take(MAX_RENDER_LINES_PER_MSG)
        .map(|line| Line::from(Span::styled(line.to_string(), Style::default().fg(theme.fg))))
        .collect()
}

/// Tool cards, Thinking rows, and read/edit file groups are "activity"
/// rows: consecutive ones render glued together with no gap row between them.
fn is_activity_msg(msg: &Msg) -> bool {
    matches!(msg, Msg::Tool(_) | Msg::FileGroup(_) | Msg::Thinking(_))
}

/// Take up to `limit` display columns worth of leading chars from `text`.
fn take_width(text: &str, limit: usize) -> (&str, &str) {
    let mut used = 0;
    let mut end = 0;
    for (idx, ch) in text.char_indices() {
        let cw = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + cw > limit {
            break;
        }
        used += cw;
        end = idx + ch.len_utf8();
    }
    text.split_at(end)
}

/// Split one line into wrapped rows at `width` columns. Ratatui's WordWrapper
/// emits a phantom empty row whenever a line is EXACTLY as wide as the area
/// (trailing whitespace flushes first), which left background-less gaps in
/// solid fill rows (user blocks, code blocks). Splitting ourselves avoids
/// that entirely.
fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width {
        return vec![line];
    }
    let base = line.style;
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in line.spans {
        let mut rest: &str = span.content.as_ref();
        loop {
            let available = width.saturating_sub(used);
            let rest_w = UnicodeWidthStr::width(rest);
            if rest_w <= available {
                if !rest.is_empty() {
                    current.push(Span::styled(rest.to_string(), span.style));
                    used += rest_w;
                }
                break;
            }
            // The next char does not fit in the remaining columns
            // (e.g. a 2-wide CJK glyph with 1 column left): flush the
            // current row first, then retry with a full row.
            let first_w = rest
                .chars()
                .next()
                .map(|c| UnicodeWidthStr::width(c.to_string().as_str()))
                .unwrap_or(0);
            if first_w > available {
                if first_w > width {
                    // A single glyph wider than the whole row can never be
                    // placed — skip it instead of looping forever.
                    if let Some(c) = rest.chars().next() {
                        rest = &rest[c.len_utf8()..];
                    } else {
                        break;
                    }
                    continue;
                }
                if !current.is_empty() {
                    out.push(Line::from(std::mem::take(&mut current)).patch_style(base));
                }
                used = 0;
                continue;
            }
            let (chunk, rem) = take_width(rest, available);
            current.push(Span::styled(chunk.to_string(), span.style));
            out.push(Line::from(std::mem::take(&mut current)).patch_style(base));
            used = 0;
            rest = rem;
        }
    }
    if !current.is_empty() {
        out.push(Line::from(current).patch_style(base));
    }
    out
}

/// Number of display rows `wrap_line` emits for one cache row. Mirrors the
/// splitter above exactly (same flush rules) so the viewport math and the
/// rendered rows can never disagree.
fn wrapped_rows(line: &Line<'static>, width: usize) -> usize {
    if width == 0 || line.width() <= width {
        return 1;
    }
    let mut rows = 0usize;
    let mut used = 0usize;
    let mut have = false;
    for span in &line.spans {
        let mut rest: &str = span.content.as_ref();
        loop {
            let available = width.saturating_sub(used);
            let rest_w = UnicodeWidthStr::width(rest);
            if rest_w <= available {
                if !rest.is_empty() {
                    have = true;
                    used += rest_w;
                }
                break;
            }
            let first_w = rest
                .chars()
                .next()
                .map(|c| UnicodeWidthStr::width(c.to_string().as_str()))
                .unwrap_or(0);
            if first_w > available {
                if first_w > width {
                    if let Some(c) = rest.chars().next() {
                        rest = &rest[c.len_utf8()..];
                    } else {
                        break;
                    }
                    continue;
                }
                if have {
                    rows += 1;
                    have = false;
                }
                used = 0;
                continue;
            }
            let (_, rem) = take_width(rest, available);
            rows += 1;
            have = false;
            used = 0;
            rest = rem;
        }
    }
    if have {
        rows += 1;
    }
    rows
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
                    line = line.patch_style(
                        Style::default().fg(theme.fg).bg(theme.bg),
                    );
                    let width = line.width();
                    if width < area_width {
                        line.push_span(Span::styled(
                            " ".repeat(area_width - width),
                            Style::default().fg(theme.fg).bg(theme.bg),
                        ));
                    }
                }
                line
            })
            .collect();
    }
    if let Msg::User { text } = msg {
        // User block: 1-row padding above/below, the configured horizontal
        // gutter on EVERY row, and text pre-wrapped inside the page width so
        // continuation rows keep the same inner padding.
        let gutter = state.config.user_input_padding;
        let avail = area_width.saturating_sub(gutter).max(1);
        let fill_row = || {
            Line::from(Span::styled(
                " ".repeat(area_width),
                Style::default().fg(theme.fg).bg(theme.bg_soft),
            ))
        };
        let mut out = vec![fill_row()];
        for line in text.lines() {
            if line.is_empty() {
                out.push(fill_row());
                continue;
            }
            for chunk in wrap_text(line, avail) {
                let mut row = Line::from(vec![
                    Span::styled(
                        " ".repeat(gutter),
                        Style::default().fg(theme.fg).bg(theme.bg_soft),
                    ),
                    Span::styled(chunk, Style::default().fg(theme.fg).bg(theme.bg_soft)),
                ]);
                let used = row.width();
                if used < area_width {
                    row.push_span(Span::styled(
                        " ".repeat(area_width - used),
                        Style::default().fg(theme.fg).bg(theme.bg_soft),
                    ));
                }
                out.push(row);
            }
        }
        out.push(fill_row());
        return out;
    }
    let mut out = Vec::new();
    for line in msg_lines(msg, state) {
        out.push(line);
    }
    out
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
    // Remember the content width for copy-mode row math (wrapped user
    // blocks count by wrapped rows).
    state.render_width = width;
    // Rebuild the base lines only when the transcript changed structurally;
    // streaming chunks splice just the tail; copy-mode overlays patch a
    // clone of the visible window each frame.
    if !state.cache_valid {
        let mut base: Vec<Line<'static>> = Vec::new();
        let mut tail_len = 0usize;
        for (idx, msg) in state.msgs.iter().enumerate() {
            let lines = styled_msg_lines(msg, state, width);
            let n = lines.len();
            base.extend(lines);
            // One-row gap between messages (and before the input bar) —
            // except between consecutive tool/file-group activity rows,
            // which stay glued with no spacing.
            let next_is_activity = state
                .msgs
                .get(idx + 1)
                .map_or(false, is_activity_msg);
            let gap = !(is_activity_msg(msg) && next_is_activity);
            tail_len = n + usize::from(gap);
            if gap {
                base.push(Line::default());
            }
        }
        state.render_cache = base;
        state.cache_tail_len = tail_len;
        state.cache_valid = true;
        state.tail_dirty = false;
    } else if state.tail_dirty {
        // Streaming chunk(s): drop the previous tail (its lines plus the
        // trailing gap) and re-render only the last message.
        let keep = state.render_cache.len().saturating_sub(state.cache_tail_len);
        state.render_cache.truncate(keep);
        if let Some(last) = state.msgs.last() {
            let lines = styled_msg_lines(last, state, width);
            state.cache_tail_len = lines.len() + 1;
            state.render_cache.extend(lines);
            state.render_cache.push(Line::default());
        } else {
            state.cache_tail_len = 0;
        }
        state.tail_dirty = false;
    }
    // History prepend: keep the viewport on the previously visible content
    // by shifting the offset by the newly added lines above it.
    if let Some(anchor) = state.prepend_line_anchor.take() {
        let delta = state.render_cache.len().saturating_sub(anchor);
        scroll.offset = scroll.offset.saturating_add(delta);
    }
    // Display-only rows (scroll-back hint) take viewport rows away from the
    // transcript window so nothing below them gets truncated.
    let len = state.render_cache.len();
    let mut available = visible;
    // Wrap-aware viewport: cache rows hold UNWRAPPED lines (a long paragraph
    // is one cache row), so the window must be chosen by DISPLAY rows, not
    // cache rows — and the wrapped overflow trimmed from the TOP. The old
    // bottom-truncation pushed the wrapped tail and the trailing gap row off
    // the screen (the input bar then sat flush against the transcript).
    let (start, show_hint) = if scroll.follow {
        // Pin the bottom: walk back from the trailing gap row until enough
        // display rows are covered; the earliest rows are the overflow.
        let mut s = len;
        let mut acc = 0usize;
        while s > 0 && acc < available {
            s -= 1;
            acc += wrapped_rows(&state.render_cache[s], width);
        }
        scroll.offset = s;
        (s, false)
    } else {
        let s = scroll.offset.min(len);
        let hint = s == 0;
        if hint {
            available = available.saturating_sub(1).max(1);
        }
        (s, hint)
    };
    let mut display: Vec<Line<'static>> = Vec::new();
    let mut i = start;
    while i < len {
        // Only wrap as much as the viewport can hold (perf: never wrap the
        // whole cache). Follow mode needs the exact window the walk picked,
        // so it keeps going to `len` and trims the front overflow below.
        if !scroll.follow && display.len() >= available {
            break;
        }
        let mut line = state.render_cache[i].clone();
        if let Some(ov) = overlay {
            let mut style = Style::default();
            if let Some((lo, hi)) = ov.sel {
                if i >= lo && i <= hi {
                    style = style.bg(theme.selection);
                }
            }
            if ov.cursor_row == i {
                style = style.fg(theme.bg).bg(theme.fg);
            }
            if style != Style::default() {
                line = line.patch_style(style);
            }
        }
        // Rows carrying a background (user blocks, code fills) keep it
        // solid: pad every wrapped row to the full width.
        for row in wrap_line(line, width) {
            let bg = row
                .style
                .bg
                .or_else(|| row.spans.iter().find_map(|s| s.style.bg));
            let mut row = row;
            if let Some(bg) = bg {
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
        i += 1;
    }
    if scroll.follow {
        // The walk over-satisfies `available` whenever a line wraps: drop
        // the excess from the TOP so the bottom rows (wrapped tail + the
        // trailing gap row before the input bar) always stay on screen.
        let excess = display.len().saturating_sub(available);
        if excess > 0 {
            display.drain(..excess);
        }
    } else {
        display.truncate(available);
    }
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
        display.insert(0, Line::from(Span::styled(hint, Style::default().fg(theme.dim))));
    }
    let paragraph = Paragraph::new(Text::from(display))
        .style(Style::default().fg(theme.fg));
    frame.render_widget(paragraph, area);
}

fn help_overlay(theme: &Theme) -> Vec<Line<'static>> {
    let rows = [
        "帮助 — e",
        "Enter 发送   Shift+Enter 换行   Alt+Enter 多行   Ctrl+Enter 发送   ↑↓ 历史/多行移行",
        "Esc 中断   Ctrl+C 清空输入/空闲退出   /exit /q /quit 退出",
        "Ctrl+B 复制模式   Ctrl+N 会话选择器   Ctrl+H 帮助",
        "/settings 设置面板   /login 登录（API key/Account/Proxy）   /new [模式] 新建会话（空格后提示可用模式）   PgUp/PgDn 滚动",
        "/theme 切换主题   /model 选择模型   /reload 重载配置/主题/技能   /skill:<名称> 注入技能",
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
) {
    let block = Block::default()
        .style(Style::default().bg(theme.bg_soft))
        // Configurable horizontal gutter + 1-row vertical padding.
        .padding(Padding::new(padding, padding, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if copy_active {
        // -- COPY -- status strip replaces the editor (design §3.1).
        let hint = Line::from(vec![
            Span::styled("-- COPY -- ", Style::default().fg(theme.user).add_modifier(Modifier::BOLD)),
            Span::styled(
                "[hjkl]移动 [V]行选 [Ctrl+V]块选 [y]复制 [Enter]展开 [Esc]退出",
                Style::default().fg(theme.dim),
            ),
        ]);
        frame.render_widget(Paragraph::new(Text::from(vec![hint])), inner);
        return;
    }

    if let Some(search) = &input.search {
        // Ctrl+R history search strip.
        let matches = input.matching_history(&search.query);
        let preview = matches
            .get(search.sel)
            .map(|m| m.chars().take(60).collect::<String>())
            .unwrap_or_default();
        let line = Line::from(vec![
            Span::styled("search: ", Style::default().fg(theme.user)),
            Span::styled(search.query.clone(), Style::default().fg(theme.fg)),
            Span::styled(" ▏ ", Style::default().fg(theme.dim)),
            Span::styled(preview, Style::default().fg(theme.dim)),
            Span::styled(
                format!("  ({}/{})", search.sel + 1, matches.len()),
                Style::default().fg(theme.dim),
            ),
        ]);
        frame.render_widget(Paragraph::new(Text::from(vec![line])), inner);
        return;
    }

    if let Some(toast_text) = toast {
        let line = Line::from(Span::styled(
            format!("❯ {toast_text}"),
            Style::default().fg(theme.ok),
        ));
        frame.render_widget(Paragraph::new(Text::from(vec![line])), inner);
        return;
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
            rendered.push(Line::from(Span::styled(
                (*text).clone(),
                Style::default().fg(theme.rose).add_modifier(Modifier::BOLD),
            )));
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
                Span::styled(before, Style::default().fg(theme.fg)),
                Span::styled(at, Style::default().fg(theme.bg).bg(theme.fg)),
                Span::styled(after, Style::default().fg(theme.fg)),
            ]));
        } else {
            rendered.push(Line::from(Span::styled(
                (*text).clone(),
                Style::default().fg(theme.fg),
            )));
        }
    }
    let paragraph = Paragraph::new(Text::from(rendered)).style(Style::default().bg(theme.bg_soft));
    frame.render_widget(paragraph, inner);
    // Place the terminal cursor into the input bar for IME-friendly input.
    // x = display width of the wrapped row up to the cursor. CJK glyphs
    // occupy two cells, so use Unicode width, not char count.
    let col = if placeholder {
        UnicodeWidthStr::width(display.as_str())
    } else {
        let (text, off) = &chunks[cursor_row];
        let before: String = text.chars().take(input.cursor.saturating_sub(*off)).collect();
        UnicodeWidthStr::width(before.as_str())
    } as u16;
    frame.set_cursor_position(Position::new(
        inner.x + col,
        inner.y + cursor_row.saturating_sub(start) as u16,
    ));
}

/// One page of transcript scrolling (used by PgUp/PgDn).
pub fn scroll_page(scroll: &mut ScrollState, area_height: usize, lines_total: usize, up: bool) {
    let page = area_height.saturating_sub(1).max(1);
    if up {
        scroll.follow = false;
        scroll.offset = scroll.offset.saturating_sub(page);
    } else {
        let max = lines_total.saturating_sub(area_height);
        let next = scroll.offset.saturating_add(page).min(max);
        scroll.offset = next;
        if next >= max {
            scroll.follow = true;
        }
    }
}

/// Session picker state (design §3.6).
pub struct PickerState {
    pub sessions: Vec<crate::protocol::SessionInfo>,
    pub query: String,
    pub sel: usize,
}

impl Default for PickerState {
    fn default() -> Self {
        Self { sessions: Vec::new(), query: String::new(), sel: 0 }
    }
}

impl PickerState {
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

/// One selectable theme in the `/theme` picker.
pub struct ThemeInfo {
    pub name: String,
    pub palette: Theme,
}

/// /theme picker state: the discovered themes, hovered by `sel`.
pub struct ThemePicker {
    pub themes: Vec<ThemeInfo>,
    pub sel: usize,
}

impl ThemePicker {
    pub fn from_themes(files: &[crate::theme::ThemeFile], current: &str) -> Self {
        let themes: Vec<ThemeInfo> = files
            .iter()
            .map(|f| ThemeInfo {
                name: f.name.clone(),
                palette: f.to_theme().unwrap_or_default(),
            })
            .collect();
        let sel = themes.iter().position(|t| t.name == current).unwrap_or(0);
        Self { themes, sel }
    }
}

/// /model picker state: providers on the left, the selected provider's
/// models on the right. ←→ switches provider, ↑↓ moves the model cursor,
/// Enter applies.
pub struct ModelPicker {
    pub providers: Vec<crate::protocol::ModelProviderInfo>,
    pub prov_sel: usize,
    pub model_sel: usize,
    /// Current selection (provider id, model id), for the initial cursor.
    pub current: Option<(String, String)>,
}

impl ModelPicker {
    pub fn new(providers: Vec<crate::protocol::ModelProviderInfo>, current: Option<(String, String)>) -> Self {
        let prov_sel = current
            .as_ref()
            .and_then(|(p, _)| providers.iter().position(|x| x.id == *p))
            .unwrap_or(0);
        let model_sel = current
            .as_ref()
            .and_then(|(_, m)| {
                providers.get(prov_sel).and_then(|x| x.models.iter().position(|y| y.id == *m))
            })
            .unwrap_or(0);
        Self { providers, prov_sel, model_sel, current }
    }

    /// The (provider id, model id) under the cursor.
    pub fn selected(&self) -> Option<(String, String)> {
        let provider = self.providers.get(self.prov_sel)?;
        let model = provider.models.get(self.model_sel)?;
        Some((provider.id.clone(), model.id.clone()))
    }
}

/// Full-screen theme picker overlay (`/theme`): each row is the theme name
/// plus a small color swatch so the palette is previewable before applying.
pub fn render_theme_picker(frame: &mut Frame, picker: &ThemePicker, theme: &Theme) {
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
        .border_style(Style::default().fg(theme.user))
        .title(" 主题 · /theme ")
        .style(Style::default().bg(theme.bg_soft));
    let inner = block.inner(rect);
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(block, rect);

    let mut rows: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            "选择配色主题（themes 目录下的合法主题）",
            Style::default().fg(theme.dim),
        )),
        Line::from(Span::styled(
            "─".repeat(width.saturating_sub(2) as usize),
            Style::default().fg(theme.dim),
        )),
    ];
    let list_height = inner.height.saturating_sub(4) as usize;
    let start = picker.sel.saturating_sub(list_height - 1);
    for (i, info) in picker.themes.iter().enumerate().skip(start).take(list_height) {
        let marker = if i == picker.sel { "› " } else { "  " };
        let mut spans = vec![
            Span::styled(
                marker.to_string(),
                Style::default().fg(theme.user),
            ),
            Span::styled(
                info.name.clone(),
                if i == picker.sel {
                    Style::default().fg(theme.bg).bg(theme.fg)
                } else {
                    Style::default().fg(theme.fg)
                },
            ),
            Span::styled("  ", Style::default().fg(theme.dim)),
        ];
        for color in [
            info.palette.bg,
            info.palette.fg,
            info.palette.user,
            info.palette.ok,
            info.palette.err,
            info.palette.running,
        ] {
            spans.push(Span::styled("  ", Style::default().bg(color)));
            spans.push(Span::styled(" ", Style::default().fg(theme.dim)));
        }
        rows.push(Line::from(spans));
    }
    if picker.themes.is_empty() {
        rows.push(Line::from(Span::styled(
            "（无可用主题）",
            Style::default().fg(theme.dim),
        )));
    }
    rows.push(Line::from(Span::styled(
        "↑↓ 选择   Enter 应用   Esc 退出",
        Style::default().fg(theme.dim),
    )));
    frame.render_widget(Paragraph::new(Text::from(rows)), inner);
}

/// Full-screen model picker overlay (`/model`): providers on the left,
/// the selected provider's models on the right. ←→ switches provider, ↑↓
/// moves the model cursor, Enter applies, Esc cancels.
pub fn render_model_picker(frame: &mut Frame, picker: &ModelPicker, theme: &Theme) {
    let area = frame.area();
    let width = (area.width * 80 / 100).clamp(50, area.width);
    let height = (area.height * 80 / 100).clamp(10, area.height);
    let rect = ratatui::layout::Rect {
        x: (area.width - width) / 2,
        y: (area.height - height) / 2,
        width,
        height,
    };
    let block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(theme.user))
        .title(" 模型 · /model ")
        .style(Style::default().bg(theme.bg_soft));
    let inner = block.inner(rect);
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(block, rect);

    // Two columns: providers (40%) | models (60%).
    let prov_w = inner.width * 40 / 100;
    let model_w = inner.width - prov_w - 1;
    let list_h = inner.height.saturating_sub(3) as usize;

    let mut prov_rows: Vec<Line<'static>> = Vec::new();
    let prov_start = picker.prov_sel.saturating_sub(list_h.saturating_sub(1));
    for (i, p) in picker.providers.iter().enumerate().skip(prov_start).take(list_h) {
        let marker = if i == picker.prov_sel { "› " } else { "  " };
        prov_rows.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.user)),
            Span::styled(
                trim_to_width(&p.name, prov_w.saturating_sub(2) as usize),
                if i == picker.prov_sel {
                    Style::default().fg(theme.bg).bg(theme.fg)
                } else {
                    Style::default().fg(theme.fg)
                },
            ),
        ]));
    }
    let mut model_rows: Vec<Line<'static>> = Vec::new();
    let models = picker.providers.get(picker.prov_sel).map(|p| p.models.as_slice()).unwrap_or(&[]);
    if models.is_empty() {
        model_rows.push(Line::from(Span::styled("（无可用模型）", Style::default().fg(theme.dim))));
    } else {
        let model_start = picker.model_sel.saturating_sub(list_h.saturating_sub(1));
        for (i, m) in models.iter().enumerate().skip(model_start).take(list_h) {
            let marker = if i == picker.model_sel { "› " } else { "  " };
            let mut spans = vec![
                Span::styled(marker.to_string(), Style::default().fg(theme.user)),
                Span::styled(
                    trim_to_width(&m.name, model_w.saturating_sub(2) as usize),
                    if i == picker.model_sel {
                        Style::default().fg(theme.bg).bg(theme.fg)
                    } else {
                        Style::default().fg(theme.fg)
                    },
                ),
            ];
            if i == picker.model_sel {
                if let Some(d) = &m.description {
                    spans.push(Span::styled(
                        format!("  {d}"),
                        Style::default().fg(theme.dim),
                    ));
                }
            }
            model_rows.push(Line::from(spans));
        }
    }

    let mut rows = vec![
        Line::from(Span::styled(
            "─".repeat(width.saturating_sub(2) as usize),
            Style::default().fg(theme.dim),
        )),
    ];
    let max_rows = prov_rows.len().max(model_rows.len());
    for i in 0..max_rows {
        let pl = prov_rows.get(i).cloned().unwrap_or_else(|| Line::from(""));
        let ml = model_rows.get(i).cloned().unwrap_or_else(|| Line::from(""));
        // Left column padded to prov_w, then a gutter, then the model column.
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.extend(pl.spans);
        let used = spans.iter().map(|s| s.width()).sum::<usize>() as u16;
        let pad = prov_w.saturating_sub(used) + 1;
        spans.push(Span::styled(" ".repeat(pad as usize), Style::default().fg(theme.dim)));
        spans.extend(ml.spans);
        rows.push(Line::from(spans));
    }
    rows.push(Line::from(Span::styled(
        "←→ 切提供商   ↑↓ 选模型   Enter 应用   Esc 退出",
        Style::default().fg(theme.dim),
    )));
    frame.render_widget(Paragraph::new(Text::from(rows)), inner);
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
        .border_style(Style::default().fg(theme.user))
        .title(" 会话选择 · Ctrl+N ")
        .style(Style::default().bg(theme.bg_soft));
    let inner = block.inner(rect);
    // Wipe the transcript cells underneath so short rows don't bleed.
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(block, rect);

    let filtered = picker.filtered();
    let mut rows: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled("> ", Style::default().fg(theme.user)),
            Span::styled(picker.query.clone(), Style::default().fg(theme.fg)),
            Span::styled("█", Style::default().fg(theme.user)),
        ]),
        Line::from(Span::styled(
            "─".repeat(width.saturating_sub(2) as usize),
            Style::default().fg(theme.dim),
        )),
    ];
    let list_height = inner.height.saturating_sub(4) as usize;
    let start = picker.sel.saturating_sub(list_height - 1);
    let window = filtered.iter().skip(start).take(list_height);
    for (i, &idx) in window.enumerate() {
        let s = &picker.sessions[idx];
        let marker = if i + start == picker.sel { "› " } else { "  " };
        let live = if s.live { "●" } else { " " };
        let title = if s.title.is_empty() { "(未命名会话)" } else { &s.title };
        let mut spans = vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.user)),
            Span::styled(format!("{live} "), Style::default().fg(if s.live { theme.ok } else { theme.dim })),
            Span::styled(
                trim_to_width(title, width.saturating_sub(28) as usize),
                if i + start == picker.sel {
                    Style::default().fg(theme.bg).bg(theme.fg)
                } else {
                    Style::default().fg(theme.fg)
                },
            ),
        ];
        spans.push(Span::styled(
            format!("  {}", &s.id[..s.id.len().min(24)]),
            Style::default().fg(theme.dim),
        ));
        rows.push(Line::from(spans));
    }
    if filtered.is_empty() {
        rows.push(Line::from(Span::styled(
            "（无匹配会话）",
            Style::default().fg(theme.dim),
        )));
    }
    rows.push(Line::from(Span::styled(
        "↑↓ 选择   Enter 切换   Esc 退出",
        Style::default().fg(theme.dim),
    )));
    frame.render_widget(Paragraph::new(Text::from(rows)), inner);
}

/// /login panel (D33): the input bar becomes a login settings page with
/// three fields — API key / 账号 / proxy. Same borderless Ash shape as the
/// settings panel: the focused NAME sits on Night; Enter opens an edit
/// (API key edits render as bullets, the value itself is never shown), and
/// every confirmed edit is sent to the bridge immediately.
pub fn render_login(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    theme: &Theme,
) {
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg_soft)), area);
    let rows = Layout::vertical([
        Constraint::Length(1), // padding
        Constraint::Length(1), // title
        Constraint::Length(1), // gap
        Constraint::Min(1),    // items
        Constraint::Length(1), // footer hint / error
    ])
    .split(area);

    let buffer = frame.buffer_mut();
    let title = match &login.page {
        crate::login::Page::Menu => "登录",
        crate::login::Page::Providers => "API key · 选择提供商",
        crate::login::Page::ApiKey { .. } => "API key · 填写",
        crate::login::Page::Account => "Account · 网页登录",
        crate::login::Page::ProxyList => "Proxy · 已保存的代理",
        crate::login::Page::ProxyForm => "Proxy · 新建代理",
    };
    buffer.set_line(
        rows[1].x,
        rows[1].y,
        &Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user).bg(theme.bg_soft)),
            Span::styled(title, Style::default().fg(theme.fg).bg(theme.bg_soft)),
        ]),
        rows[1].width,
    );

    let item_area = rows[3];
    match &login.page {
        crate::login::Page::Menu => {
            let items: &[(&str, &str)] = &[
                ("API key", "为各模型提供商填写 API key"),
                ("Account", "网页登录 Codex（ChatGPT 订阅）"),
                ("Proxy", "管理自定义代理端点"),
            ];
            for (i, (label, hint)) in items.iter().enumerate() {
                let y = item_area.y + i as u16;
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
                    Span::styled((*hint).to_string(), Style::default().fg(theme.dim).bg(theme.bg_soft)),
                    theme,
                );
            }
        }
        crate::login::Page::Providers => {
            for (i, p) in login.providers.iter().enumerate() {
                let y = item_area.y + i as u16;
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
                    i == login.pos,
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
            for (i, p) in login.proxies.iter().enumerate() {
                let y = item_area.y + i as u16;
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
                    Span::styled(p.base_url.clone(), Style::default().fg(theme.dim).bg(theme.bg_soft)),
                    theme,
                );
            }
            let new_y = item_area.y + login.proxies.len() as u16;
            if new_y < item_area.y + item_area.height {
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
            for (i, label) in fields.iter().enumerate() {
                let y = item_area.y + i as u16;
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
                        2 => crate::login::PROTOCOLS[login.draft.protocol % crate::login::PROTOCOLS.len()].1.to_string(),
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
                login_list_row(buffer, area, y, i == login.pos, editing, label, value, theme);
            }
            let save_y = item_area.y + crate::login::PROXY_ROWS as u16 - 1;
            if save_y < item_area.y + item_area.height {
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
            crate::login::Page::ProxyList => "↑/↓ 选择   Enter 新建   Esc 返回".to_string(),
            crate::login::Page::ProxyForm => "↑/↓ 选字段   Enter 编辑/切换   Enter 保存   Esc 返回".to_string(),
        }
    };
    let fg = if login.error.is_some() { theme.err } else { theme.dim };
    buffer.set_line(
        rows[4].x,
        rows[4].y,
        &Line::from(Span::styled(footer, Style::default().fg(fg).bg(theme.bg_soft))),
        rows[4].width,
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
    let name_bg = if focused && !editing { theme.bg } else { theme.bg_soft };
    let label_width = UnicodeWidthStr::width(label);
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled("  ", Style::default().fg(theme.fg).bg(name_bg)),
        Span::styled(label.to_string(), Style::default().fg(theme.fg).bg(name_bg)),
    ];
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
    // Borderless panel in place of the input bar: Ash background by default;
    // Night marks the focused item's name (descriptions stay Ash).
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg_soft)), area);
    // One row of padding above and below the category tabs: the nav bar
    // keeps a blank row between itself and the content below.
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    // ---- category tabs: centered, not selectable; ←/→ switches pages ----
    {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, cat) in crate::settings::CATEGORIES.iter().enumerate() {
            let fg = if i == settings.category { theme.ok } else { theme.fg };
            spans.push(Span::styled(
                format!(" {cat} "),
                Style::default().fg(fg).bg(theme.bg_soft),
            ));
            spans.push(Span::styled(
                "  ",
                Style::default().fg(theme.fg).bg(theme.bg_soft),
            ));
        }
        let line = Line::from(spans);
        let width = (line.width() as u16).min(rows[1].width);
        let x = rows[1].x + rows[1].width.saturating_sub(width) / 2;
        let buffer = frame.buffer_mut();
        buffer.set_line(x, rows[1].y, &line, width);
    }

    // ---- two columns with a 2-space gap: small name/description column
    // ---- left, values right.
    let cols = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Length(2),
        Constraint::Min(1),
    ])
    .split(rows[3]);
    let left_width = cols[0].width as usize;
    let value_width = cols[2].width;
    let items = crate::settings::items_in(settings.category);
    let focus_idx = Some(settings.pos[settings.category]);
    let mut left_rows: Vec<Line<'static>> = Vec::new();
    let mut right_rows: Vec<Line<'static>> = Vec::new();
    // (item index, first row, row count) for scroll anchoring.
    let mut anchors: Vec<(usize, usize, usize)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let focused = focus_idx == Some(idx);
        // While editing, the focus moves to the value: the name loses the
        // Night highlight.
        let editing = focused && settings.editing.is_some();
        let name_bg = if focused && !editing { theme.bg } else { theme.bg_soft };
        let first = left_rows.len();
        // Name row: fg text; the focused NAME fills with Night (the
        // description below is never selected).
        let mut name_line = Line::from(vec![
            Span::styled("  ", Style::default().fg(theme.fg).bg(name_bg)),
            Span::styled((*item).label, Style::default().fg(theme.fg).bg(name_bg)),
        ]);
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
        for chunk in wrap_text(item.desc, left_width.saturating_sub(2).max(1)) {
            left_rows.push(Line::from(vec![
                Span::styled(
                    "  ",
                    Style::default().fg(theme.dim).bg(theme.bg_soft),
                ),
                Span::styled(
                    chunk,
                    Style::default().fg(theme.dim).bg(theme.bg_soft),
                ),
            ]));
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
    let visible = rows[3].height as usize;
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
        "←/→ 分类   ↑/↓ 选择   Enter 编辑   Esc 退出 · 即改即存"
    };
    let buffer = frame.buffer_mut();
    buffer.set_line(
        rows[4].x,
        rows[4].y,
        &Line::from(Span::styled(
            hint,
            Style::default().fg(theme.dim).bg(theme.bg_soft),
        )),
        rows[4].width,
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
    let editing_input = focused_row
        && matches!(settings.editing, Some(crate::settings::Edit::Input { .. }));
    let editing_choice = match settings.editing {
        Some(crate::settings::Edit::Choice { cursor }) if focused_row => Some(cursor),
        _ => None,
    };
    let cell_bg = if editing_input { theme.bg } else { theme.bg_soft };
    let line: Line<'static> = match item.kind {
        crate::settings::ItemKind::Choice { options } => {
            let current = (item.get)(config);
            let selected = options
                .iter()
                .position(|o| (*o).starts_with(current.as_str()));
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, opt) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("  ", Style::default().fg(theme.fg).bg(cell_bg)));
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
                spans.push(Span::styled(format!("{glyph} {opt}"), Style::default().fg(fg).bg(bg)));
            }
            Line::from(spans)
        }
        crate::settings::ItemKind::ModeChoice | crate::settings::ItemKind::ThemeChoice => {
            // Same ○/● rendering over the live roster (the current value is
            // appended when the roster no longer lists it).
            let options = crate::settings::dynamic_options(item, config, &settings.modes, &settings.themes);
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, opt) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("  ", Style::default().fg(theme.fg).bg(cell_bg)));
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
                spans.push(Span::styled(format!("{glyph} {opt}"), Style::default().fg(fg).bg(bg)));
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
            let mut input_line = Line::from(Span::styled(
                text,
                Style::default().fg(fg).bg(cell_bg),
            ));
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
    use unicode_width::UnicodeWidthStr;
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

    fn state_with(msgs: Vec<Msg>) -> AppState {
        let mut s = AppState::default();
        s.msgs = msgs;
        s
    }

    #[test]
    fn user_block_has_vertical_padding() {
        let s = state_with(vec![Msg::User { text: "你好".into() }]);
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
        let s = state_with(vec![Msg::User { text: "a\nb\nc".into() }]);
        let lines = styled_msg_lines(&s.msgs[0], &s, 80);
        assert_eq!(lines.len(), 5);
    }

    /// Wrapped continuation rows keep the configured horizontal gutter.
    #[test]
    fn user_block_wrapped_rows_keep_gutter() {
        let s = state_with(vec![Msg::User { text: "x".repeat(30) }]);
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

    /// Tool summaries and read/edit file lists use bark (#6f5d63, the `dim`
    /// theme slot) instead of the default white text color.
    #[test]
    fn activity_text_uses_bark_dim() {
        use crate::model::{EditItem, FileGroup, ReadItem, ToolCard, ToolState};

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
        assert_eq!(summary.style.fg, Some(theme.dim), "tool summary uses bark (dim)");

        let mut s2 = AppState::default();
        s2.config = config;
        s2.msgs.push(Msg::FileGroup(FileGroup {
            reads: vec![ReadItem {
                call_id: "r1".into(),
                file: "src/a.rs".into(),
                ok: Some(true),
            }],
            edits: vec![EditItem {
                call_id: "e1".into(),
                file: "src/b.rs".into(),
                ok: Some(true),
            }],
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
        assert_eq!(
            bullet.style.fg,
            Some(theme.ok),
            "success bullet uses green"
        );

        // Failure: bullet turns red (and only then).
        let mut s3 = AppState::default();
        let mut fail_cfg = crate::config::Config::default();
        fail_cfg.resolved_theme = Theme::ferra();
        s3.config = fail_cfg;
        s3.msgs.push(Msg::FileGroup(FileGroup {
            reads: vec![ReadItem {
                call_id: "r2".into(),
                file: "src/x.rs".into(),
                ok: Some(false),
            }],
            edits: vec![],
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
        assert_eq!(
            bullet.style.fg,
            Some(theme.err),
            "failure bullet uses red"
        );
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
            state: ToolState::Done { ok: true, lines: 3, duration_ms: 0 },
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
        assert!(text.starts_with("  • "), "done glyph is a spaced bullet: {text}");
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
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0..80)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        // Layout: transcript 34, input 3 (y 34..37), spacer y=37, status
        // y=38, title row y=39.
        let spacer = row(37);
        assert!(spacer.trim().is_empty(), "one-row gap above the status bar: {spacer:?}");
        let status = row(38);
        assert!(status.contains('•'), "working bullet present: {status:?}");
        assert!(
            !status.contains("idle") && !status.contains("running"),
            "no idle/running text label: {status:?}"
        );
        // The bullet leads the status bar, flush against the content area's
        // left edge (x=4 with the default page margin), followed by one space
        // and the `e` brand.
        assert!(
            status.trim_start().starts_with("• e"),
            "bullet leads, space-separated from e: {status:?}"
        );
        // Char index (not byte index — the row holds multi-byte `·`): each
        // cell contributes exactly one char, so this is also the cell column.
        let bullet_x = status.chars().position(|c| c == '•').expect("bullet cell");
        assert_eq!(bullet_x, 4, "bullet at the content area's leading edge");
        assert_eq!(
            buf[(bullet_x as u16, 38)].fg,
            theme.dim,
            "idle bullet is gray"
        );
        // Running (with no visible activity and `working` left false): the
        // bullet still leaves gray (breathing toward yellow) — the running
        // status alone must drive the breath.
        s.status = AgentStatus::Running;
        s.activity_epoch = Some(std::time::Instant::now() - std::time::Duration::from_millis(
            (crate::model::BREATH_CYCLE_MS / 2) as u64,
        ));
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
            .unwrap();
        let backend_fg = terminal.backend().buffer()[(bullet_x as u16, 38)].fg;
        assert_ne!(backend_fg, theme.dim, "running bullet breathes (not gray)");
    }

    /// The title row below the status bar shows the session's latest
    /// session/title; blank until the session has one.
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
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
            .unwrap();
        // Fixed bottom: input(3, y18..21) + gap(y21) + status(y22) + title(y23).
        let title_row = |buf: &ratatui::buffer::Buffer, y: u16| -> String {
            (0u16..80)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        };
        let title = title_row(terminal.backend().buffer(), 23);
        assert!(title.replace(' ', "").contains("重构bridge"), "title row: {title}");
        assert!(
            title_row(terminal.backend().buffer(), 22).contains("• e"),
            "status bar still one row above the title"
        );
        // Without a title the row is blank (no leftover glyphs).
        s.session_title = None;
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
            .unwrap();
        let blank = title_row(terminal.backend().buffer(), 23);
        assert!(blank.trim().is_empty(), "blank title row: {blank:?}");
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
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
            .unwrap();
        let buf = terminal.backend().buffer();
        let title = (0u16..80)
            .map(|x| buf[(x, 23)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>();
        // Left-aligned title (❯ prefix); CJK wide glyphs occupy two cells,
        // so strip spaces before matching.
        assert!(title.replace(' ', "").starts_with("❯重构bridge"), "title left: {title:?}");
        // Right-aligned path (all ASCII) sits flush against the content
        // area's right edge. The page leaves a 4-column margin on each side
        // of an 80-col terminal, so the 72-wide content area ends at x=75.
        assert!(title.trim_end().ends_with(r"D:\MyProjects\Chore\dsh"), "path right: {title:?}");
        assert_eq!(buf[(75, 23)].symbol(), "h", "path ends at the content's right edge");

        // A very long title truncates with an ellipsis, keeping the path.
        s.session_title = Some("x".repeat(120));
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
            .unwrap();
        let buf2 = terminal.backend().buffer();
        let long = (0u16..80)
            .map(|x| buf2[(x, 23)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>();
        assert!(long.contains('…'), "long title truncates with an ellipsis: {long:?}");
        assert!(long.trim_end().ends_with(r"D:\MyProjects\Chore\dsh"), "path stays visible: {long:?}");
        assert_eq!(buf2[(75, 23)].symbol(), "h", "path still flush right");
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
                    crate::protocol::QuestionOption { label: "乙".into(), description: None },
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
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
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
        assert!(text(5).contains("甲的说明"), "option description: {}", row(5));
        let bar = row(7);
        assert!(bar.contains("◄") && bar.contains("►"), "selection bar: {bar}");
        assert!(bar.contains('甲') && bar.contains('乙'), "options visible: {bar}");
        // The highlighted option renders reversed (Night on Mist).
        let x = (0u16..80).find(|&x| buf[(x, 7)].symbol() == "甲").expect("甲 cell");
        let cell = &buf[(x, 7)];
        assert_eq!(cell.fg, Theme::ferra().bg, "selected option fg = Night");
        assert_eq!(cell.bg, Theme::ferra().fg, "selected option bg = Mist");
        // The right-side hint carries the confirm affordance.
        assert!(bar.replace(' ', "").contains("Enter确定"), "confirm hint: {bar}");
        // Status bar marks the wait.
        assert!(text(10).contains("等待选择"), "status: {}", row(10));
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
        s.queue = vec![
            "这是马上要发的队列中的提示词".into(),
            "x".repeat(100),
        ];
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
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
        assert!(!compact(17).ends_with('x'), "truncated row stays on one line");
        // Night background + Bark foreground on the queue text.
        let cell = &buf[(4, 16)];
        assert_eq!(cell.bg, theme.bg, "queue row bg = Night");
        assert_eq!(cell.fg, theme.dim, "queue row fg = Bark");
        // The queue must not spill into the input bar below.
        assert_eq!(buf[(4, 18)].bg, theme.bg_soft, "input bar untouched below the queue");
        assert!(!row(18).contains('x'), "no overflow into the input bar");
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
                .flat_map(|y| (0..80u16).map(move |x| buf[(x, y)].symbol().chars().next().unwrap_or(' ')))
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
        assert!(status_row.contains("• e"), "status bar remains above the title row");
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
        assert_eq!(buf[tab_cell].bg, theme.bg_soft, "tabs are not selectable (Ash)");
        // Focused name cell: Night background; descriptions stay Ash.
        assert!(has_cell(buf, '主', theme.fg, theme.bg), "focused name cell is Night");
        assert!(
            has_cell(buf, 'C', theme.dim, theme.bg_soft),
            "description is Bark on Ash"
        );
        // Selected choice option: green ● on Ash.
        assert!(has_cell(buf, '●', theme.ok, theme.bg_soft), "selected option ● is green");
        // Unselected choice option: ○ + default fg on Ash.
        assert!(has_cell(buf, '○', theme.fg, theme.bg_soft), "unselected option ○ is default fg");
        // Unfocused value text stays on Ash.
        assert!(has_cell(buf, 'c', theme.fg, theme.bg_soft), "unfocused value stays Ash");
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
                .flat_map(|y| (0..80u16).map(move |x| buf[(x, y)].symbol().chars().next().unwrap_or(' ')))
                .collect::<String>()
                .replace(' ', "")
        };
        // Menu page: the three choices render.
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: Some(&mut login) }))
            .unwrap();
        let all = all_text(terminal.backend().buffer());
        assert!(all.contains("登录"), "login title");
        assert!(all.contains("APIkey"), "API key menu item (spaces stripped)");
        assert!(all.contains("Account"), "Account menu item");
        assert!(all.contains("Proxy"), "Proxy menu item");
        // Provider sub-page: the provider row shows the configured-key view.
        login.page = crate::login::Page::Providers;
        login.loading = false;
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: Some(&mut login) }))
            .unwrap();
        let all = all_text(terminal.backend().buffer());
        assert!(all.contains("DeepSeek"), "provider row: {all:?}");
        assert!(all.contains("已配置…1234"), "configured key hint");
        assert!(!all.contains("sk-"), "no secret on screen");
        // API-key edit masks the typed secret as bullets.
        login.page = crate::login::Page::ApiKey { provider: "deepseek".into(), buf: String::new() };
        login.editing = Some("sk-secret".into());
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: Some(&mut login) }))
            .unwrap();
        let all = all_text(terminal.backend().buffer());
        assert!(all.contains("●●●●●●●●●█"), "typed key renders as bullets");
        assert!(!all.contains("sk-secret"), "typed secret never renders in plain text");
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
                text: format!("第{i}行 用户消息内容 很长 很长 很长"),
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
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
            .unwrap();
        let buf = terminal.backend().buffer();
        // Layout: 40 rows = transcript(35) + input(3) + spacer(1) + status(1)
        // → input.y = 35. Borderless popup: 8 matches → rect y = 35 - 10 = 25.
        let rect = ratatui::layout::Rect::new(4, 25, 46, 10);
        for y in rect.y..rect.y + rect.height {
            let row: String = (rect.x..rect.x + rect.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect();
            // The transcript behind uses these CJK characters; any of them
            // inside the popup means text bled through.
            assert!(
                !row.contains('消') && !row.contains('很') && !row.contains('长'),
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
        terminal
            .draw(|frame| {
                render_input(
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
            terminal.get_cursor_position().unwrap(),
            Position::new(8, 1),
            "cursor tracks CJK display width"
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
        let action = input.handle_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            true,
        );
        assert_eq!(action, InputAction::None, "Shift+Enter must not send");
        assert_eq!(input.buf, "\n");
        assert!(input.multiline);
        assert_eq!(input_rows(&input), 2, "trailing newline is a second row");

        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = Theme::ferra();
        terminal
            .draw(|frame| {
                render_input(
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
            terminal.get_cursor_position().unwrap(),
            Position::new(2, 2),
            "cursor sits on the new empty row"
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
        terminal
            .draw(|f| {
                render_input(
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
            terminal.get_cursor_position().unwrap(),
            Position::new(20, 3),
            "cursor follows the wrapped rows to the end"
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
        s.msgs.push(Msg::User { text: "x".repeat(150) });
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
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
    fn tail_splice_updates_only_the_streaming_message() {        use ratatui::backend::TestBackend;

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
        let len1 = s.render_cache.len();
        // Append a chunk: cache stays valid, only the tail is dirty.
        s.apply_event(&serde_json::json!({
            "type": "assistant/chunk", "seq": 3, "time": 0,
            "data": {"chunk": {"type": "text-delta", "text": "式"}}
        }));
        assert!(s.cache_valid && s.tail_dirty);
        terminal
            .draw(|f| render_transcript(f, area, &mut s, &mut scroll, &theme, false, None))
            .unwrap();
        // Streaming tail replaced (1 line + gap): same total length.
        assert_eq!(s.render_cache.len(), len1, "tail splice keeps the line count");
        let joined: String = s
            .render_cache
            .iter()
            .flat_map(|l| l.spans.iter().map(|sp| sp.content.as_ref()))
            .collect();
        assert!(joined.contains("流式"), "spliced tail shows the full stream");
        assert!(!s.tail_dirty, "tail dirty flag cleared after render");
    }

    /// History prepend: the viewport must stay anchored on the previously
    /// visible content — the scroll offset shifts by the lines added above.
    #[test]
    fn history_prepend_keeps_viewport() {        use ratatui::backend::TestBackend;

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
        let old_len = s.render_cache.len();
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
        let delta = s.render_cache.len() - old_len;
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
            assert!(!row.contains("前"), "older history is above the viewport: {row}");
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
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
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
        assert_eq!(buf[(19, 0)].bg, ratatui::style::Color::Reset, "left of the page is empty");
        assert_eq!(buf[(60, 0)].bg, ratatui::style::Color::Reset, "right of the page is empty");
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
        let text: String = lines[0].spans.iter().map(|sp| sp.content.as_ref()).collect();
        assert_eq!(text, "  • Thinking...", "tool-card shape");
        let bullet = &lines[0].spans[1];
        assert_eq!(bullet.content, "•");
        assert!(bullet.style.fg.is_some(), "running bullet is colored (breathing)");
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
        let text: String = lines[0].spans.iter().map(|sp| sp.content.as_ref()).collect();
        assert_eq!(text, "  • Thinking... x5", "consecutive phases show a count");
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
                    line: Line::from(Span::styled("  code · 1 行", Style::default().fg(Theme::ferra().dim))),
                    unit: 0,
                    raw_line: Some(0),
                    atomic: true,
                    fill: true,
                },
                RenderLine {
                    line: Line::from(Span::styled("  code", Style::default().fg(Theme::ferra().fg))),
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

    /// Consecutive activity rows (tool cards + read/edit groups) render
    /// glued together — no gap row between them.
    #[test]
    fn tool_and_file_group_are_glued_without_gap() {
        use ratatui::backend::TestBackend;
        use crate::model::{EditItem, FileGroup, ToolCard, ToolState};

        let tool = || Msg::Tool(ToolCard {
            call_id: "b1".into(),
            name: "bash".into(),
            summary: "cmd".into(),
            state: ToolState::Running,
            frame: 0,
            start_ms: 0,
            done_since: None,
            done_from: None,
        });
        let group = || Msg::FileGroup(FileGroup {
            reads: vec![],
            edits: vec![EditItem {
                call_id: "e1".into(),
                file: "a.rs".into(),
                ok: None,
            }],
            frame: 0,
            done_since: None,
            done_from: None,
        });

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
        assert_eq!(s.render_cache.len(), 3);
        assert!(
            s.render_cache[1].width() > 0,
            "no gap row between tool and file group"
        );
        assert_eq!(s.render_cache[2].width(), 0, "gap before the input bar");

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
        assert_eq!(s2.render_cache.len(), 6);
        assert_eq!(s2.render_cache[3].width(), 0, "gap kept between user and tool");
        assert!(s2.render_cache[4].width() > 0);
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
            assert_eq!(split, count, "row count mismatch at width {width}: {line:?}");
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
        s.msgs.push(Msg::User { text: "你好世界".into() });
        let input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let backend = TestBackend::new(80, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render(f, &mut s, &input, &mut scroll, &theme, RenderOverlays { help_visible: false, overlay: None, toast: None, settings: None, login: None }))
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
        use crate::model::{EditItem, FileGroup, ReadItem};

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config;
        s.msgs.push(Msg::FileGroup(FileGroup {
            reads: vec![
                ReadItem {
                    call_id: "r1".into(),
                    file: "src/ok.rs".into(),
                    ok: Some(true),
                },
                ReadItem {
                    call_id: "r2".into(),
                    file: "src/bad.rs".into(),
                    ok: Some(false),
                },
            ],
            edits: vec![EditItem {
                call_id: "e1".into(),
                file: "src/fix.rs".into(),
                ok: Some(false),
            }],
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
        assert_eq!(text[0], "  • read ok.rs", "failed items stay out of the fold");
        assert!(text.iter().any(|l| l.contains("bad.rs")), "failed read listed: {text:?}");
        assert!(text.iter().any(|l| l.contains("fix.rs")), "failed edit listed: {text:?}");
        assert_eq!(lines.len(), 3, "fold + one line per failed item");
        let theme = Theme::ferra();
        let merged_bullet = lines[0]
            .spans
            .iter()
            .find(|sp| sp.content == "•")
            .expect("merged bullet");
        assert_eq!(merged_bullet.style.fg, Some(theme.ok), "merged bullet green");
        for line in &lines[1..] {
            let b = line.spans.iter().find(|sp| sp.content == "•").expect("fail bullet");
            assert_eq!(b.style.fg, Some(theme.err), "failed bullets red");
        }
    }

    /// Repeated reads/edits of the same file collapse into `name xN` on the
    /// group lines; failed repeats stay one line per distinct file (matching
    /// `file_group_line_count`).
    #[test]
    fn file_group_collapses_repeats_into_counts() {
        use crate::model::{EditItem, FileGroup, ReadItem};

        let mut config = crate::config::Config::default();
        // ui tests assert the ferra palette — pin the resolved theme so the
        // default (deepseek-e) doesn't shift the expected colors.
        config.resolved_theme = Theme::ferra();
        let mut s = AppState::default();
        s.config = config;
        s.msgs.push(Msg::FileGroup(FileGroup {
            reads: vec![
                ReadItem { call_id: "r1".into(), file: "src/foo.rs".into(), ok: Some(true) },
                ReadItem { call_id: "r2".into(), file: "src/foo.rs".into(), ok: Some(true) },
                ReadItem { call_id: "r3".into(), file: "src/foo.rs".into(), ok: Some(true) },
            ],
            edits: vec![
                EditItem { call_id: "e1".into(), file: "src/model.rs".into(), ok: Some(true) },
                EditItem { call_id: "e2".into(), file: "src/model.rs".into(), ok: Some(true) },
                EditItem { call_id: "e3".into(), file: "src/bar.rs".into(), ok: Some(true) },
                EditItem { call_id: "e4".into(), file: "world.rs".into(), ok: Some(true) },
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
            "  • read foo.rs x3; edit model.rs x2, bar.rs, world.rs",
            "repeats fold into xN"
        );
        // Failed repeats: one red line per DISTINCT file, with its count.
        let group = FileGroup {
            reads: vec![
                ReadItem { call_id: "r1".into(), file: "src/bad.rs".into(), ok: Some(false) },
                ReadItem { call_id: "r2".into(), file: "src/bad.rs".into(), ok: Some(false) },
                ReadItem { call_id: "r3".into(), file: "src/bad.rs".into(), ok: Some(false) },
            ],
            edits: vec![],
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
        assert_eq!(text, "  • read bad.rs x3");
        assert_eq!(
            crate::model::file_group_line_count(&group),
            1,
            "copy-mode row math matches the collapsed line"
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
