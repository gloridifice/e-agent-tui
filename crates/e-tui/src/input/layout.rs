use super::InputDisplay;
use crate::wrap::{wrap_text_chunks, WrapChunk};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn text_width(area_width: u16, padding: u16) -> usize {
    usize::from(area_width.saturating_sub(padding.saturating_mul(2).saturating_add(1))).max(1)
}

pub(crate) struct InputLayout {
    pub chunks: Vec<WrapChunk>,
    pub cursor_row: usize,
}

impl InputLayout {
    pub fn new(display: &InputDisplay, width: usize) -> Self {
        let mut chunks = Vec::new();
        let mut offset = 0;
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
                for mut chunk in wrap_text_chunks(line, width.max(1)) {
                    chunk.start += offset;
                    chunk.end += offset;
                    chunks.push(chunk);
                }
            }
            offset += line.chars().count() + 1;
        }
        let mut layout = Self {
            chunks,
            cursor_row: 0,
        };
        layout.cursor_row = layout.row_for_cursor(display.cursor);
        layout
    }

    pub fn row_for_cursor(&self, cursor: usize) -> usize {
        for (row, chunk) in self.chunks.iter().enumerate() {
            if cursor < chunk.start {
                return row.saturating_sub(1);
            }
            if cursor < chunk.end {
                return row;
            }
        }
        self.chunks.len() - 1
    }

    pub fn cursor_column(&self, cursor: usize) -> usize {
        let chunk = &self.chunks[self.row_for_cursor(cursor)];
        let before: String = chunk
            .text
            .chars()
            .take(cursor.saturating_sub(chunk.start))
            .collect();
        UnicodeWidthStr::width(before.as_str())
    }

    pub fn cursor_at_column(&self, row: usize, column: usize) -> usize {
        let chunk = &self.chunks[row];
        let mut cursor = chunk.start;
        let mut width = 0;
        for grapheme in chunk.text.graphemes(true) {
            let next = cursor + grapheme.chars().count();
            width += UnicodeWidthStr::width(grapheme);
            let next_row_owns_cursor = self
                .chunks
                .get(row + 1)
                .is_some_and(|following| following.start == next);
            if width > column || next_row_owns_cursor {
                break;
            }
            cursor = next;
        }
        cursor
    }
}
