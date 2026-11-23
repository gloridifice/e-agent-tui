use ratatui::buffer::Buffer;

use crate::{
    mouse_selection::{SelectionCellRange, SelectionJoin},
    wrap::LayoutLine,
};

#[derive(Debug, Default)]
pub(crate) struct SelectionLayout {
    previous: Option<PresentedRow>,
    joins: Vec<(PresentedRow, PresentedRow, String)>,
}

#[derive(Debug, Clone)]
struct PresentedRow {
    x: u16,
    y: u16,
    text: String,
    content: SelectionCellRange,
}

impl SelectionLayout {
    pub fn push(&mut self, x: u16, y: u16, row: &LayoutLine) {
        let width = row.line.width();
        let start = row.copy.content.start.min(width) as u16;
        let end = row.copy.content.end.min(width) as u16;
        let next = PresentedRow {
            x,
            y,
            text: row
                .line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect(),
            content: SelectionCellRange {
                x: x.saturating_add(start),
                y,
                width: end.saturating_sub(start),
            },
        };
        if let (Some(previous), Some(separator)) = (&self.previous, &row.copy.join_before) {
            if previous.x == x && previous.y.checked_add(1) == Some(y) {
                self.joins
                    .push((previous.clone(), next.clone(), separator.clone()));
            }
        }
        self.previous = Some(next);
    }

    pub fn append(&mut self, mut other: Self) {
        self.joins.append(&mut other.joins);
        self.previous = None;
    }

    pub fn collect(&self, buffer: &Buffer, owns: impl Fn(u16, u16) -> bool) -> Vec<SelectionJoin> {
        self.joins
            .iter()
            .filter(|(previous, next, _)| {
                previous.matches(buffer, &owns) && next.matches(buffer, &owns)
            })
            .map(|(previous, next, separator)| SelectionJoin {
                previous: previous.content,
                next: next.content,
                separator: separator.clone(),
            })
            .collect()
    }
}

impl PresentedRow {
    fn matches(&self, buffer: &Buffer, owns: &impl Fn(u16, u16) -> bool) -> bool {
        use ratatui::buffer::CellWidth;
        use unicode_segmentation::UnicodeSegmentation;
        let mut x = self.x;
        for glyph in self.text.graphemes(true) {
            if !buffer.area.contains((x, self.y).into()) || !owns(x, self.y) {
                return false;
            }
            let cell = &buffer[(x, self.y)];
            if cell.symbol() != glyph || cell.modifier.contains(ratatui::style::Modifier::HIDDEN) {
                return false;
            }
            x = x.saturating_add(cell.cell_width().max(1));
        }
        x <= buffer.area.right()
    }
}
