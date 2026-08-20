use super::*;

pub(super) fn render_input(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    input: &InputState,
    theme: &Theme,
    toast: Option<&str>,
    padding: u16,
) -> Option<Position> {
    let block = Block::default()
        .style(theme.input.background.style())
        // Configurable horizontal gutter + 1-row vertical padding.
        .padding(Padding::new(padding, padding, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
    // The visible window is exactly the text area height (the box grows with
    // wrapped rows up to INPUT_MAX_ROWS); the cursor row is always kept in
    // view when content overflows the window.
    let total = chunks.len();
    let mut cursor_row = 0usize;
    for (i, (text, off)) in chunks.iter().enumerate() {
        if placeholder {
            cursor_row = i; // the cursor renders after the block
        } else if input.cursor >= *off && input.cursor <= off + text.chars().count() {
            cursor_row = i;
        }
    }
    let visible_rows = (inner.height as usize).max(1);
    let start = if total <= visible_rows {
        0
    } else {
        cursor_row
            .saturating_sub(visible_rows - 1)
            .min(total - visible_rows)
    };
    let end = (start + visible_rows).min(total);

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
