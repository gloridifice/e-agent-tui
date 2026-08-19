use super::*;

pub(super) fn render_status(
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
    let drafting = state.new_conversation.is_some();
    let bullet = if !drafting && (state.status == AgentStatus::Running || state.working) {
        Span::styled(
            "•",
            Style::default().fg(breathing_color(theme, state.breath_phase())),
        )
    } else {
        Span::styled("•", dim)
    };
    let mode = state
        .new_conversation
        .as_ref()
        .map(|draft| draft.mode.as_str())
        .or(state.current_mode.as_deref())
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
    if let Some(rate) = (!drafting).then(|| state.cache_hit_rate()).flatten() {
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
pub(super) fn render_title(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &AppState,
    theme: &Theme,
) {
    let style = theme.input.status_hint.style();
    frame.render_widget(ratatui::widgets::Clear, area);
    let buffer = frame.buffer_mut();
    let width = area.width as usize;

    // Left-aligned title, truncated to leave the path (plus a small gap)
    // visible. Drawn first so the path below wins any overlap (defensive:
    // the truncation already reserves the path's columns).
    let title = if state.new_conversation.is_some() {
        "新对话".to_owned()
    } else {
        match state.session_title.as_deref() {
            Some(title) if !title.trim().is_empty() => title.trim().to_owned(),
            _ => "新会话".to_owned(),
        }
    };
    let cwd = if state.new_conversation.is_some() {
        String::new()
    } else {
        state
            .session_cwd
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string()
    };
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
