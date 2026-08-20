//! Width-aware greedy word wrapping shared by transcript, preview, cards, and
//! the input bar.
//!
//! The algorithm fills each row with as many whole words as fit (greedy
//! first-fit), breaks at the whitespace that precedes an overflowing word, and
//! only falls back to grapheme splitting when one word is wider than a whole
//! row. Display widths always use `unicode_width`; grapheme clusters such as
//! combining marks, emoji ZWJ sequences, and flags are never split.

use std::ops::Range;

use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy)]
struct Token {
    start: usize,
    end: usize,
    width: usize,
    space: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapChunk {
    pub text: String,
    /// Character index of the first emitted source character.
    pub start: usize,
    /// Character index one past the last emitted source character.
    pub end: usize,
    /// Byte range of the emitted source slice. Dropped break whitespace is a
    /// gap between `byte_end` of one chunk and `byte_start` of the next.
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Split `text` into rows of at most `width` display columns using greedy
/// word wrapping. Rows are emitted byte ranges; whitespace consumed by a row
/// break is intentionally absent from both neighboring ranges.
fn word_wrap_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut builder = RowBuilder {
        text,
        width,
        rows: Vec::new(),
        start: 0,
        end: 0,
        used: 0,
        has_row: false,
    };
    let mut pending_space: Option<Token> = None;

    for token in tokens {
        if token.space {
            if builder.has_row {
                // Defer spaces before a word so a break can consume them
                // instead of leaving a trailing space on the previous row.
                pending_space = Some(token);
            } else {
                builder.emit_fitted(token);
            }
        } else if let Some(space) = pending_space.take() {
            builder.emit_after_separator(space, token);
        } else {
            builder.emit_word(token);
        }
    }

    if let Some(space) = pending_space.take() {
        if builder.has_row && builder.used + space.width <= width {
            builder.emit(space.start, space.end, space.width);
        } else {
            builder.end_row();
            builder.emit_fitted(space);
        }
    }
    builder.end_row();
    builder.rows
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    for (index, grapheme) in text.grapheme_indices(true) {
        let end = index + grapheme.len();
        let space = grapheme.chars().all(char::is_whitespace);
        let width = UnicodeWidthStr::width(grapheme);
        if let Some(last) = tokens.last_mut() {
            if last.space == space && last.end == index {
                last.end = end;
                last.width += width;
                continue;
            }
        }
        tokens.push(Token {
            start: index,
            end,
            width,
            space,
        });
    }
    tokens
}

struct RowBuilder<'a> {
    text: &'a str,
    width: usize,
    rows: Vec<Range<usize>>,
    start: usize,
    end: usize,
    used: usize,
    has_row: bool,
}

impl RowBuilder<'_> {
    fn emit(&mut self, start: usize, end: usize, width: usize) {
        if !self.has_row {
            self.start = start;
        }
        self.end = end;
        self.used += width;
        self.has_row = true;
    }

    fn end_row(&mut self) {
        if !self.has_row {
            return;
        }
        self.rows.push(self.start..self.end);
        self.start = 0;
        self.end = 0;
        self.used = 0;
        self.has_row = false;
    }

    /// Append a whole word to the current row, or break first when it does
    /// not fit. Over-wide words fall back to grapheme splitting.
    fn emit_word(&mut self, word: Token) {
        if word.width > self.width {
            self.end_row();
            self.emit_fitted(word);
            return;
        }
        if self.has_row && self.used + word.width > self.width {
            self.end_row();
        }
        self.emit(word.start, word.end, word.width);
    }

    /// Append `space` and `word` to the current row. When the full whitespace
    /// run fits it is preserved; otherwise the whole run is consumed by the
    /// row break.
    fn emit_after_separator(&mut self, space: Token, word: Token) {
        debug_assert!(self.has_row);
        if word.width > self.width {
            self.end_row();
            self.emit_fitted(word);
            return;
        }
        if self.used + space.width + word.width <= self.width {
            self.emit(space.start, space.end, space.width);
            self.emit(word.start, word.end, word.width);
            return;
        }
        // The whitespace run is the break opportunity; consume all of it.
        self.end_row();
        self.emit(word.start, word.end, word.width);
    }

    /// Grapheme fallback for tokens wider than a whole row (and for over-wide
    /// whitespace runs). Every emitted row stays within `width`, except a
    /// single grapheme wider than `width` occupies its own row.
    fn emit_fitted(&mut self, token: Token) {
        for (offset, grapheme) in self.text[token.start..token.end].grapheme_indices(true) {
            let start = token.start + offset;
            let end = start + grapheme.len();
            let grapheme_width = UnicodeWidthStr::width(grapheme);
            if grapheme_width > self.width {
                self.end_row();
                self.emit(start, end, grapheme_width);
                self.end_row();
                continue;
            }
            if self.has_row && self.used + grapheme_width > self.width {
                self.end_row();
            }
            self.emit(start, end, grapheme_width);
            if self.used == self.width {
                self.end_row();
            }
        }
    }
}

pub fn wrap_text_chunks(text: &str, width: usize) -> Vec<WrapChunk> {
    let ranges = word_wrap_ranges(text, width);
    let mut chunks = Vec::with_capacity(ranges.len());
    let mut byte_pos = 0usize;
    let mut char_pos = 0usize;
    for range in ranges {
        if byte_pos < range.start {
            char_pos += text[byte_pos..range.start].chars().count();
        }
        let start = char_pos;
        let source = &text[range.clone()];
        char_pos += source.chars().count();
        chunks.push(WrapChunk {
            text: source.to_owned(),
            start,
            end: char_pos,
            byte_start: range.start,
            byte_end: range.end,
        });
        byte_pos = range.end;
    }
    chunks
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    wrap_text_chunks(text, width)
        .into_iter()
        .map(|chunk| chunk.text)
        .collect()
}

/// Split one styled line into wrapped rows without Ratatui's exact-width
/// phantom row.
pub fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width {
        return vec![line];
    }
    let base = line.style;
    let mut text = String::new();
    let mut styles = Vec::with_capacity(line.spans.len());
    for span in &line.spans {
        let start = text.len();
        text.push_str(span.content.as_ref());
        styles.push((start, text.len(), span.style));
    }

    let ranges = word_wrap_ranges(&text, width);
    if ranges.is_empty() {
        return vec![Line::default().patch_style(base)];
    }
    ranges
        .into_iter()
        .map(|range| {
            let mut spans = Vec::new();
            for &(style_start, style_end, style) in &styles {
                let from = range.start.max(style_start);
                let to = range.end.min(style_end);
                if from < to {
                    spans.push(Span::styled(text[from..to].to_owned(), style));
                }
            }
            Line::from(spans).patch_style(base)
        })
        .collect()
}

pub fn wrapped_rows(line: &Line<'static>, width: usize) -> usize {
    if width == 0 || line.width() <= width {
        return 1;
    }
    let mut text = String::new();
    for span in &line.spans {
        text.push_str(span.content.as_ref());
    }
    word_wrap_ranges(&text, width).len().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn greedy_wrap_breaks_between_words() {
        assert_eq!(
            wrap_text("aa bb cc", 3),
            vec!["aa".to_string(), "bb".to_string(), "cc".to_string()]
        );
        assert_eq!(wrap_text("aaa bb", 4), vec!["aaa", "bb"]);
        assert_eq!(wrap_text("aaa b", 5), vec!["aaa b"]);
    }

    #[test]
    fn greedy_wrap_preserves_spaces_when_they_fit() {
        assert_eq!(wrap_text("aa  bb", 6), vec!["aa  bb"]);
        // The whitespace run is consumed by the break when it no longer fits.
        assert_eq!(wrap_text("aa  bb", 5), vec!["aa", "bb"]);
    }

    #[test]
    fn greedy_wrap_hard_breaks_overwide_words() {
        assert_eq!(wrap_text("abcdef", 3), vec!["abc", "def"]);
        assert_eq!(wrap_text("a bcdef", 4), vec!["a", "bcde", "f"]);
    }

    #[test]
    fn greedy_wrap_keeps_grapheme_clusters_intact() {
        let family = "👨‍👩‍👧";
        assert_eq!(wrap_text(family, 2), vec![family.to_string()]);
        assert_eq!(
            wrap_text("e\u{301}x", 1),
            vec!["e\u{301}".to_string(), "x".to_string()]
        );
        assert_eq!(wrap_text("你好世界", 4), vec!["你好", "世界"]);
    }

    #[test]
    fn chunks_track_character_offsets_across_dropped_spaces() {
        let chunks = wrap_text_chunks("aa bb cc", 3);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| (chunk.text.as_str(), chunk.start, chunk.end))
                .collect::<Vec<_>>(),
            vec![("aa", 0, 2), ("bb", 3, 5), ("cc", 6, 8)]
        );
        assert_eq!(chunks[0].byte_end, 2);
        assert_eq!(chunks[1].byte_start, 3);
    }

    #[test]
    fn styled_wrap_keeps_styles_across_word_and_grapheme_breaks() {
        use ratatui::style::Color;

        let line = Line::from(vec![
            Span::styled("aa", Style::default().fg(Color::Red)),
            Span::styled(" bb", Style::default().fg(Color::Blue)),
        ]);
        let rows = wrap_line(line, 2);
        let text: Vec<String> = rows
            .iter()
            .map(|row| row.spans.iter().map(|span| span.content.as_ref()).collect())
            .collect();
        assert_eq!(text, vec!["aa", "bb"]);
        assert_eq!(rows[0].spans[0].style.fg, Some(Color::Red));
        assert_eq!(rows[1].spans[0].style.fg, Some(Color::Blue));

        let grapheme_line = Line::from(vec![
            Span::styled("e", Style::default().fg(Color::Red)),
            Span::styled("\u{301}👩", Style::default().fg(Color::Blue)),
            Span::styled("\u{200d}💻x", Style::default().fg(Color::Green)),
        ]);
        let rows = wrap_line(grapheme_line, 2);
        let text: Vec<String> = rows
            .iter()
            .map(|row| row.spans.iter().map(|span| span.content.as_ref()).collect())
            .collect();
        assert_eq!(text, vec!["e\u{301}", "👩\u{200d}💻", "x"]);
    }

    #[test]
    fn row_count_matches_materialized_word_wraps() {
        for width in 1..=8 {
            let line = Line::from("aa bb 中cdef");
            assert_eq!(
                wrapped_rows(&line, width),
                wrap_line(line.clone(), width).len()
            );
        }
    }
}
