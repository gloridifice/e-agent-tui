//! Width-aware greedy line wrapping shared by transcript, preview, cards, and
//! the input bar.
//!
//! The algorithm fills each row with as many whole words as fit (greedy
//! first-fit), breaks at the whitespace that precedes an overflowing word, and
//! only falls back to grapheme splitting when one word is wider than a whole
//! row. A "word" is a maximal run of graphemes with no line break opportunity
//! between them: whitespace runs and, per the Unicode Line Breaking Algorithm
//! (UAX #14), runs such as Latin words, glued punctuation pairs, and Hangul
//! syllable blocks stay whole, while CJK ideographs, kana, and Hangul
//! syllables each offer a break opportunity. Display widths always use
//! `unicode_width`; grapheme clusters such as combining marks, emoji ZWJ
//! sequences, and flags are never split.

use std::ops::Range;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
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

/// Split `text` into tokens: whitespace runs and non-whitespace segments.
/// Consecutive whitespace graphemes merge into one space token; non-whitespace
/// graphemes merge into a word token only when UAX #14 forbids a break at that
/// boundary, so a word is a maximal run with no internal break opportunity.
///
/// Break opportunities are consumed in lockstep with the grapheme iterator, so
/// no membership structure is materialized per call. Opportunities that fall
/// inside a grapheme cluster (before the next grapheme boundary) are skipped.
fn tokenize(text: &str) -> Vec<Token> {
    use unicode_linebreak::{linebreaks, BreakOpportunity};

    let mut breaks = linebreaks(text)
        .filter(|&(_, opportunity)| opportunity == BreakOpportunity::Allowed)
        .map(|(index, _)| index)
        .peekable();
    let mut tokens: Vec<Token> = Vec::new();
    for (index, grapheme) in text.grapheme_indices(true) {
        let end = index + grapheme.len();
        let space = grapheme.chars().all(char::is_whitespace);
        let width = UnicodeWidthStr::width(grapheme);
        while breaks.peek().is_some_and(|offset| *offset < index) {
            breaks.next();
        }
        let break_allowed = breaks.peek() == Some(&index);
        let merge = matches!(
            tokens.last(),
            Some(last)
                if last.space == space && last.end == index && (space || !break_allowed)
        );
        if merge {
            let last = tokens.last_mut().expect("tokens.last() matched above");
            last.end = end;
            last.width += width;
        } else {
            tokens.push(Token {
                start: index,
                end,
                width,
                space,
            });
        }
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

/// Number of graphemes at the start of an append-only streaming line whose
/// placement cannot be changed by extending its open trailing wrap atom.
///
/// The deferred separator and final non-space token stay held. For an
/// over-wide token, complete hard-wrapped rows are stable and may be admitted;
/// only its final partial row remains held. This deliberately reuses the same
/// UAX #14 tokenization as `word_wrap_ranges`.
pub fn stable_wrap_prefix_graphemes(text: &str, width: usize) -> usize {
    let tokens = tokenize(text);
    let Some(last) = tokens.last().copied() else {
        return 0;
    };
    let held_start = if last.space {
        last.start
    } else {
        tokens
            .get(tokens.len().saturating_sub(2))
            .filter(|token| token.space)
            .map_or(last.start, |space| space.start)
    };
    if last.space || width == 0 || last.width <= width {
        return text[..held_start].graphemes(true).count();
    }

    let mut used = 0usize;
    let mut stable_end = last.start;
    for (offset, grapheme) in text[last.start..last.end].grapheme_indices(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if grapheme_width > width {
            stable_end = last.start + offset + grapheme.len();
            used = 0;
            continue;
        }
        if used + grapheme_width > width {
            used = 0;
        }
        used += grapheme_width;
        if used == width {
            stable_end = last.start + offset + grapheme.len();
            used = 0;
        }
    }
    let admitted_end = if stable_end > last.start {
        stable_end
    } else {
        held_start
    };
    text[..admitted_end].graphemes(true).count()
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

/// Clip a styled line to the display width without adding a marker. Grapheme
/// boundaries are computed over the concatenated line, so a combining mark or
/// ZWJ sequence split across adjacent style spans remains one unit.
pub fn clip_line(line: Line<'static>, width: usize) -> Line<'static> {
    clip_line_with_marker(line, width, None)
}

/// Clip a styled line and append one ellipsis when it overflows. The marker
/// consumes one display column and uses the last retained span style.
pub fn ellipsize_line(line: Line<'static>, width: usize) -> Line<'static> {
    clip_line_with_marker(line, width, Some("…"))
}

fn clip_line_with_marker(line: Line<'static>, width: usize, marker: Option<&str>) -> Line<'static> {
    let base = line.style;
    if width == 0 {
        return Line::default().patch_style(base);
    }
    if line.width() <= width {
        return line;
    }
    let marker_width = marker.map_or(0, UnicodeWidthStr::width);
    let budget = width.saturating_sub(marker_width);
    let mut text = String::new();
    let mut styles = Vec::with_capacity(line.spans.len());
    for span in &line.spans {
        let start = text.len();
        text.push_str(span.content.as_ref());
        styles.push((start, text.len(), span.style));
    }
    let mut used = 0usize;
    let mut spans = Vec::new();
    for (start, grapheme) in text.grapheme_indices(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if used + grapheme_width > budget {
            break;
        }
        used += grapheme_width;
        let style = styles
            .iter()
            .find(|(from, to, _)| *from <= start && start < *to)
            .map(|(_, _, style)| *style)
            .unwrap_or(base);
        push_merged_span(&mut spans, grapheme.to_owned(), base.patch(style));
    }
    if let Some(marker) = marker {
        let style = spans.last().map_or(base, |span: &Span<'static>| span.style);
        spans.push(Span::styled(marker.to_owned(), style));
    }
    Line::from(spans).patch_style(base)
}

pub fn clip_text(text: &str, width: usize) -> String {
    let line = clip_line(Line::from(text.to_owned()), width);
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

pub fn ellipsize_text(text: &str, width: usize) -> String {
    let line = ellipsize_line(Line::from(text.to_owned()), width);
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

fn push_merged_span(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(&text);
            return;
        }
    }
    spans.push(Span::styled(text, style));
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
    fn cjk_ideographs_offer_break_opportunities() {
        assert_eq!(wrap_text("你好世界", 2), vec!["你", "好", "世", "界"]);
        assert_eq!(wrap_text("你好世界", 4), vec!["你好", "世界"]);
        assert_eq!(wrap_text("你好世界", 6), vec!["你好世", "界"]);
        assert_eq!(wrap_text("你好世界", 8), vec!["你好世界"]);
        // Mixed CJK + Latin: the Latin word stays whole on its own row.
        assert_eq!(wrap_text("世界abc", 5), vec!["世界", "abc"]);
        assert_eq!(wrap_text("好abc", 4), vec!["好", "abc"]);
        assert_eq!(wrap_text("abc好", 4), vec!["abc", "好"]);
    }

    #[test]
    fn kinsoku_punctuation_never_starts_a_row() {
        // Fullwidth comma is CL: it glues to the preceding ideograph, so it
        // can never start a row.
        assert_eq!(wrap_text("你好，世界", 4), vec!["你", "好，", "世界"]);
        assert_eq!(wrap_text("你好，世界", 6), vec!["你好，", "世界"]);
        assert_eq!(wrap_text("你好！世界", 4), vec!["你", "好！", "世界"]);
        // Closing brackets/marks never start a row; opening brackets never end
        // one.
        let rows = wrap_text("（你好）世界！", 4);
        assert_eq!(rows, vec!["（你", "好）", "世", "界！"]);
        for row in &rows {
            assert!(
                !row.starts_with(['）', '！']),
                "row {row:?} starts with forbidden punctuation"
            );
            assert!(
                !row.ends_with('（'),
                "row {row:?} ends with an opening bracket"
            );
        }
        assert_eq!(wrap_text("「你好」世界", 6), vec!["「你", "好」世", "界"]);
    }

    #[test]
    fn small_kana_never_starts_a_row() {
        // Small kana (CJ → NS) glues to the preceding grapheme.
        assert_eq!(wrap_text("ああっあ", 4), vec!["あ", "あっ", "あ"]);
    }

    #[test]
    fn hangul_syllable_blocks_stay_together() {
        // Jamo of one syllable form a single grapheme cluster (UAX #29), so
        // the block never splits regardless of width...
        assert_eq!(wrap_text("한", 1), vec!["한"]);
        assert_eq!(wrap_text("한", 2), vec!["한"]);
        // ...while separate syllables break only between themselves (LB31).
        assert_eq!(wrap_text("가나", 2), vec!["가", "나"]);
        assert_eq!(wrap_text("가나", 4), vec!["가나"]);
    }

    #[test]
    fn numeric_and_symbol_contexts_do_not_split_internally() {
        // 1 × . × 5 (LB13 × IS, LB25 IS × NU): the number stays together.
        assert_eq!(wrap_text("1.5", 3), vec!["1.5"]);
        assert_eq!(wrap_text("1.5", 2), vec!["1.", "5"]);
        // No break before a hyphen, break allowed after it.
        assert_eq!(wrap_text("a-b", 2), vec!["a-", "b"]);
    }

    #[test]
    fn chunks_track_character_offsets_across_cjk_breaks() {
        let chunks = wrap_text_chunks("你好，世界", 4);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| (chunk.text.as_str(), chunk.start, chunk.end))
                .collect::<Vec<_>>(),
            vec![("你", 0, 1), ("好，", 1, 3), ("世界", 3, 5)]
        );
        assert_eq!(chunks[0].byte_end, 3);
        assert_eq!(chunks[1].byte_start, 3);
        assert_eq!(chunks[2].byte_end, 15);
    }

    #[test]
    fn row_count_matches_materialized_word_wraps() {
        for width in 1..=8 {
            let line = Line::from("aa bb 中cdef，你好「世界」abc");
            assert_eq!(
                wrapped_rows(&line, width),
                wrap_line(line.clone(), width).len()
            );
        }
    }

    #[test]
    fn stable_stream_prefix_holds_open_words_and_deferred_spaces() {
        assert_eq!(stable_wrap_prefix_graphemes("hello wor", 10), 5);
        assert_eq!(stable_wrap_prefix_graphemes("hello world", 10), 5);
        assert_eq!(stable_wrap_prefix_graphemes("hello world ", 10), 11);
        assert_eq!(stable_wrap_prefix_graphemes("hello world n", 10), 11);
    }

    #[test]
    fn stable_stream_prefix_respects_cjk_punctuation_atoms() {
        assert_eq!(stable_wrap_prefix_graphemes("你好", 4), 1);
        assert_eq!(stable_wrap_prefix_graphemes("你好，", 4), 1);
        assert_eq!(stable_wrap_prefix_graphemes("你好，世", 4), 3);
    }

    #[test]
    fn stable_stream_prefix_releases_complete_overwide_rows() {
        assert_eq!(stable_wrap_prefix_graphemes("abcdefghij", 4), 8);
        assert_eq!(stable_wrap_prefix_graphemes("aa abcdefghi", 4), 11);
        assert_eq!(stable_wrap_prefix_graphemes("", 4), 0);
    }
}
