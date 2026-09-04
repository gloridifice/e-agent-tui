use super::*;
use crate::wrap::{wrap_text_chunks, WrapChunk};

pub(super) fn render_input(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    input: &InputState,
    theme: &Theme,
    padding: u16,
) -> Option<Position> {
    let bark = Style::default().fg(theme.input.hint.fg);
    render_ruled_chrome(frame, area, theme);
    let horizontal = padding.saturating_mul(2).saturating_add(1);
    let inner = ratatui::layout::Rect::new(
        area.x.saturating_add(padding).saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(horizontal),
        area.height.saturating_sub(2),
    );
    if inner.height > 0 && area.width > 0 {
        frame.render_widget(
            Paragraph::new("❯").style(bark),
            ratatui::layout::Rect::new(area.x, inner.y, 1, 1),
        );
    }
    let surface_style = Style::default();

    if let Some(search) = &input.search {
        // Ctrl+R history search strip.
        let matches = input.matching_history(&search.query);
        let preview = matches
            .get(search.sel)
            .map(|m| m.chars().take(60).collect::<String>())
            .unwrap_or_default();
        let line = Line::from(vec![
            Span::styled(
                crate::i18n::tr(input.language, "input.search"),
                theme.input.prompt.style(),
            ),
            Span::styled(search.query.clone(), theme.input.text.style()),
            Span::styled(" ▏ ", theme.input.hint.style()),
            Span::styled(preview, theme.input.hint.style()),
            Span::styled(
                format!("  ({}/{})", search.sel + 1, matches.len()),
                theme.input.hint.style(),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(Text::from(vec![line])).style(surface_style),
            inner,
        );
        return None;
    }

    let display = input.display_text();
    // Wrap every display line at the inner width so long content stays
    // inside the input box; each chunk remembers its source character range
    // within `display.text`. Word wrapping may consume whitespace at a row
    // break, so ranges can have gaps.
    let wrap_w = (inner.width as usize).max(1);
    let mut chunks: Vec<WrapChunk> = Vec::new();
    let mut offset = 0usize;
    for line in display.text.split('\n') {
        if line.is_empty() {
            chunks.push(WrapChunk {
                text: String::new(),
                start: offset,
                end: offset,
                byte_start: 0,
                byte_end: 0,
            });
        } else {
            for mut chunk in wrap_text_chunks(line, wrap_w) {
                chunk.start += offset;
                chunk.end += offset;
                chunks.push(chunk);
            }
        }
        offset += line.chars().count() + 1; // the newline itself
    }
    if chunks.is_empty() {
        chunks.push(WrapChunk {
            text: String::new(),
            start: 0,
            end: 0,
            byte_start: 0,
            byte_end: 0,
        });
    }
    // The visible window is exactly the text area height (the box grows with
    // wrapped rows up to INPUT_MAX_ROWS); the cursor row is always kept in
    // view when content overflows the window.
    let total = chunks.len();
    let mut cursor_row = chunks.len().saturating_sub(1);
    for (i, chunk) in chunks.iter().enumerate() {
        if display.cursor < chunk.start {
            cursor_row = i.checked_sub(1).unwrap_or(0);
            break;
        }
        if display.cursor < chunk.end || (display.cursor == chunk.end && i + 1 == chunks.len()) {
            cursor_row = i;
            break;
        }
        // Cursor exactly at a shared boundary: the next chunk owns it.
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

    // Split a row segment into styled spans: characters inside a paste
    // placeholder use the placeholder tone, everything else the text tone.
    let styled_spans = |text: &str, off: usize| -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut current = String::new();
        let mut current_placeholder = display.is_paste_char(off);
        for (i, c) in text.chars().enumerate() {
            let is_placeholder = display.is_paste_char(off + i);
            if is_placeholder != current_placeholder {
                let style = if current_placeholder {
                    theme.input.placeholder.style()
                } else {
                    theme.input.text.style()
                };
                spans.push(Span::styled(std::mem::take(&mut current), style));
                current_placeholder = is_placeholder;
            }
            current.push(c);
        }
        if !current.is_empty() {
            let style = if current_placeholder {
                theme.input.placeholder.style()
            } else {
                theme.input.text.style()
            };
            spans.push(Span::styled(current, style));
        }
        spans
    };

    let mut rendered: Vec<Line<'static>> = Vec::new();
    // When the cursor sits just past the final character of a row that already
    // fills `wrap_w`, appending the synthetic cursor space would make Ratatui
    // wrap that space into the next physical row (often the box's bottom
    // padding). Render the text without that space and patch the cursor cell
    // into the right gutter after the paragraph instead.
    let mut cursor_patch: Option<(u16, u16)> = None;
    for i in start..end {
        let chunk = &chunks[i];
        let text = &chunk.text;
        let off = chunk.start;
        // No prompt prefix: the input bar text starts flush at the edge.
        if i == cursor_row {
            // Draw the cursor on its wrapped row. A cursor in whitespace
            // consumed by a row break clamps to the end of the previous row.
            let cur = display.cursor.saturating_sub(off).min(text.chars().count());
            if text.chars().nth(cur).is_none()
                && UnicodeWidthStr::width(text.as_str()).saturating_add(1) > wrap_w
            {
                rendered.push(Line::from(styled_spans(text, off)));
                cursor_patch = Some((
                    (i - start) as u16,
                    UnicodeWidthStr::width(text.as_str()) as u16,
                ));
                continue;
            }
            let before: String = text.chars().take(cur).collect();
            let at = text.chars().nth(cur);
            let after: String = text.chars().skip(cur + 1).collect();
            let mut spans = styled_spans(&before, off);
            if let Some(at) = at {
                spans.push(Span::styled(at.to_string(), theme.input.cursor.style()));
            } else {
                spans.push(Span::styled("█", synthetic_cursor_style(theme)));
            }
            spans.extend(styled_spans(&after, off + cur + 1));
            rendered.push(Line::from(spans));
        } else {
            rendered.push(Line::from(styled_spans(text, off)));
        }
    }
    let paragraph = Paragraph::new(Text::from(rendered)).style(surface_style);
    frame.render_widget(paragraph, inner);
    if let Some((row, col)) = cursor_patch {
        let x = inner.x.saturating_add(col);
        let y = inner.y.saturating_add(row);
        if x < area.right() && y < area.bottom() {
            if let Some(cell) = frame.buffer_mut().cell_mut(Position::new(x, y)) {
                cell.set_symbol("█")
                    .set_style(synthetic_cursor_style(theme));
            }
        }
    }
    // Place the terminal cursor into the input bar for IME-friendly input.
    // x = display width of the wrapped row up to the cursor. CJK glyphs
    // occupy two cells, so use Unicode width, not char count.
    let chunk = &chunks[cursor_row];
    let before: String = chunk
        .text
        .chars()
        .take(display.cursor.saturating_sub(chunk.start))
        .collect();
    let col = UnicodeWidthStr::width(before.as_str()) as u16;
    Some(Position::new(
        inner.x + col,
        inner.y + cursor_row.saturating_sub(start) as u16,
    ))
}

// A foreground-only glyph avoids background-color trails when the cursor moves.
fn synthetic_cursor_style(theme: &Theme) -> Style {
    let fill = theme.input.cursor.bg.unwrap_or(theme.input.cursor.fg);
    Style::default().fg(fill)
}

fn render_ruled_chrome(frame: &mut Frame, area: ratatui::layout::Rect, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let top = area.y;
    let bottom = area.bottom().saturating_sub(1);
    render_rule(frame, area, top, theme);
    if bottom != top {
        render_rule(frame, area, bottom, theme);
    }
}

pub(super) fn render_rule(frame: &mut Frame, area: ratatui::layout::Rect, y: u16, theme: &Theme) {
    if area.width == 0 || y < area.y || y >= area.bottom() {
        return;
    }
    for offset in 0..area.width {
        let color = if offset < 2 || offset >= area.width.saturating_sub(2) {
            theme.diff.separator.fg
        } else {
            theme.input.hint.fg
        };
        if let Some(cell) = frame
            .buffer_mut()
            .cell_mut(Position::new(area.x.saturating_add(offset), y))
        {
            cell.set_symbol("─").set_fg(color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, PromptImage};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn synthetic_cursor_uses_a_printed_cell_and_clears_after_wide_char_deletion() {
        let theme = Theme::ferra();
        let mut input = InputState::new(&Config::default());
        input.buf = "你好世界".into();
        input.cursor = input.buf.chars().count();
        let mut terminal = Terminal::new(TestBackend::new(16, 3)).unwrap();

        terminal
            .draw(|frame| {
                render_input(frame, frame.area(), &input, &theme, 0);
            })
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(9, 1)].symbol(), "█");
        assert_eq!(terminal.backend().buffer()[(9, 1)].bg, Color::Reset);

        input.buf.pop();
        input.cursor -= 1;
        terminal
            .draw(|frame| {
                render_input(frame, frame.area(), &input, &theme, 0);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(7, 1)].symbol(), "█");
        assert_eq!(buffer[(9, 1)].symbol(), " ");
        assert_ne!(buffer[(9, 1)].bg, theme.input.cursor.bg.unwrap());
    }

    #[test]
    fn image_attachment_renders_as_one_placeholder_styled_block() {
        let theme = Theme::ferra();
        let mut input = InputState::new(&Config::default());
        input.paste_image(PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some("clip.png".into()),
        });
        let mut terminal = Terminal::new(TestBackend::new(48, 3)).unwrap();
        terminal
            .draw(|frame| {
                render_input(frame, frame.area(), &input, &theme, 1);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = (0..48).map(|x| buffer[(x, 1)].symbol()).collect::<String>();
        assert!(
            row.starts_with("❯ [Image clip.png]"),
            "rendered row: {row:?}"
        );
        assert_eq!(
            buffer[(2, 1)].fg,
            theme.input.placeholder.fg,
            "the complete block uses the atomic-placeholder role"
        );
    }
}
