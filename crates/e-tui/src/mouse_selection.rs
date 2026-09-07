//! Application-owned mouse selection over the last committed screen cells.

use std::sync::Arc;

use crate::event::PointerEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cell {
    symbol: String,
    owner: u16,
    width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Point {
    row: u16,
    column: u16,
}

/// Terminal-neutral, screen-bounded text from a successfully submitted frame.
#[derive(Debug, Clone, Default)]
pub struct SelectionFrame {
    epoch: u64,
    context: u64,
    viewport: (u16, u16),
    cells: Arc<[Cell]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionUpdate {
    pub changed: bool,
    pub copy: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionCellRange {
    pub x: u16,
    pub y: u16,
    pub width: u16,
}

impl SelectionFrame {
    pub fn for_viewport(width: u16, height: u16) -> Self {
        Self {
            viewport: (width, height),
            cells: (0..height)
                .flat_map(|_| {
                    (0..width).map(|column| Cell {
                        symbol: " ".into(),
                        owner: column,
                        width: 1,
                    })
                })
                .collect(),
            ..Self::default()
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn set_epoch(&mut self, epoch: u64) {
        self.epoch = epoch;
    }

    pub fn context(&self) -> u64 {
        self.context
    }

    pub(crate) fn set_context(&mut self, context: u64) {
        self.context = context;
    }

    pub fn same_geometry(&self, other: &Self) -> bool {
        self.context == other.context
            && self.viewport == other.viewport
            && self.cells == other.cells
    }

    pub fn matches_viewport(&self, width: u16, height: u16) -> bool {
        self.viewport == (width, height)
    }

    pub(crate) fn put_glyph(&mut self, column: u16, row: u16, symbol: &str, width: u16) {
        if row >= self.viewport.1 || column >= self.viewport.0 {
            return;
        }
        let width = width.max(1).min(self.viewport.0 - column);
        let start = usize::from(row) * usize::from(self.viewport.0) + usize::from(column);
        let cells = Arc::make_mut(&mut self.cells);
        cells[start] = Cell {
            symbol: symbol.into(),
            owner: column,
            width,
        };
        for cell in &mut cells[start + 1..start + usize::from(width)] {
            *cell = Cell {
                symbol: String::new(),
                owner: column,
                width: 0,
            };
        }
    }

    fn cell(&self, column: u16, row: u16) -> &Cell {
        &self.cells[usize::from(row) * usize::from(self.viewport.0) + usize::from(column)]
    }

    fn point_at(&self, column: u16, row: u16) -> Option<Point> {
        (column < self.viewport.0 && row < self.viewport.1).then_some(Point { row, column })
    }

    fn clamp_point(&self, column: u16, row: u16) -> Option<Point> {
        self.point_at(
            column.min(self.viewport.0.saturating_sub(1)),
            row.min(self.viewport.1.saturating_sub(1)),
        )
    }

    pub(crate) fn selected_cell_ranges(
        &self,
        selection: &MouseSelection,
    ) -> Vec<SelectionCellRange> {
        let Some((start, end)) = selection.bounds_for(self) else {
            return Vec::new();
        };
        (start.row..=end.row)
            .map(|row| {
                let first = if row == start.row { start.column } else { 0 };
                let last = if row == end.row {
                    end.column
                } else {
                    self.viewport.0 - 1
                };
                let x = self.cell(first, row).owner;
                let last_owner = self.cell(last, row).owner;
                let right = last_owner + self.cell(last_owner, row).width;
                SelectionCellRange {
                    x,
                    y: row,
                    width: right - x,
                }
            })
            .collect()
    }

    fn extract(&self, selection: &MouseSelection) -> Option<String> {
        let text = self
            .selected_cell_ranges(selection)
            .into_iter()
            .map(|range| {
                let mut line = String::new();
                for column in range.x..range.x + range.width {
                    line.push_str(&self.cell(column, range.y).symbol);
                }
                line.truncate(line.trim_end_matches(' ').len());
                line
            })
            .collect::<Vec<_>>()
            .join("\n");
        text.chars().any(|c| !c.is_whitespace()).then_some(text)
    }
}

/// Interaction state holds coordinates and identity, never a screen buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MouseSelection {
    epoch: Option<u64>,
    anchor: Option<Point>,
    focus: Option<Point>,
    dragging: bool,
    moved: bool,
}

impl MouseSelection {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn holds(&self, frame: &SelectionFrame) -> bool {
        self.dragging && self.epoch == Some(frame.epoch())
    }

    pub fn handle(&mut self, event: PointerEvent, frame: &SelectionFrame) -> SelectionUpdate {
        let before = self.clone();
        let mut copy = None;
        if self.epoch.is_some_and(|epoch| epoch != frame.epoch()) {
            self.clear();
        }
        match event {
            PointerEvent::Wheel { .. } | PointerEvent::FocusLost => self.clear(),
            PointerEvent::PrimaryPress { column, row } => {
                self.clear();
                if frame.epoch() != 0 {
                    if let Some(point) = frame.point_at(column, row) {
                        self.epoch = Some(frame.epoch());
                        self.anchor = Some(point);
                        self.focus = Some(point);
                        self.dragging = true;
                    }
                }
            }
            PointerEvent::PrimaryDrag { column, row }
            | PointerEvent::PrimaryRelease { column, row } => {
                if self.dragging {
                    self.focus = frame.clamp_point(column, row);
                    self.moved |= self.focus != self.anchor;
                    if matches!(event, PointerEvent::PrimaryRelease { .. }) {
                        self.dragging = false;
                        copy = frame.extract(self);
                    }
                }
            }
        }
        SelectionUpdate {
            changed: *self != before,
            copy,
        }
    }

    fn bounds_for(&self, frame: &SelectionFrame) -> Option<(Point, Point)> {
        if self.epoch != Some(frame.epoch()) || !self.moved {
            return None;
        }
        let (anchor, focus) = (self.anchor?, self.focus?);
        Some((anchor.min(focus), anchor.max(focus)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    fn frame(lines: &[&str]) -> SelectionFrame {
        let mut frame = SelectionFrame::for_viewport(20, lines.len() as u16);
        frame.set_epoch(1);
        for (row, line) in lines.iter().enumerate() {
            let mut column = 0;
            for glyph in line.graphemes(true) {
                let width = glyph.width() as u16;
                frame.put_glyph(column, row as u16, glyph, width);
                column += width;
            }
        }
        frame
    }

    fn drag(
        frame: &SelectionFrame,
        start: (u16, u16),
        end: (u16, u16),
    ) -> (MouseSelection, Option<String>) {
        let mut selection = MouseSelection::default();
        selection.handle(
            PointerEvent::PrimaryPress {
                column: start.0,
                row: start.1,
            },
            frame,
        );
        let copied = selection
            .handle(
                PointerEvent::PrimaryRelease {
                    column: end.0,
                    row: end.1,
                },
                frame,
            )
            .copy;
        (selection, copied)
    }

    #[test]
    fn screen_ranges_include_adjacent_panes_in_both_directions() {
        let frame = frame(&["left  │ right", "bottom"]);
        let forward = drag(&frame, (1, 0), (2, 1)).1;
        assert_eq!(forward.as_deref(), Some("eft  │ right\nbot"));
        assert_eq!(drag(&frame, (2, 1), (1, 0)).1, forward);
    }

    #[test]
    fn wide_combining_and_zwj_graphemes_are_emitted_once() {
        let frame = frame(&["A界🙂e\u{301}👩‍💻Z"]);
        let (selection, copied) = drag(&frame, (2, 0), (7, 0));
        assert_eq!(copied.as_deref(), Some("界🙂e\u{301}👩‍💻"));
        assert_eq!(
            frame.selected_cell_ranges(&selection),
            vec![SelectionCellRange {
                x: 1,
                y: 0,
                width: 7
            }]
        );
        assert_eq!(drag(&frame, (1, 0), (2, 0)).1.as_deref(), Some("界"));
    }

    #[test]
    fn visual_spaces_and_blank_rows_have_one_policy() {
        let frame = frame(&["  a  b\u{a0}  ", "", "  end "]);
        assert_eq!(
            drag(&frame, (0, 0), (8, 2)).1.as_deref(),
            Some("  a  b\u{a0}\n\n  end")
        );
        assert_eq!(drag(&frame, (0, 0), (2, 0)).1.as_deref(), Some("  a"));
        assert!(drag(&frame, (15, 0), (19, 1)).1.is_none());
    }

    #[test]
    fn drag_clamps_to_viewport_not_to_text() {
        let frame = frame(&["first", "last"]);
        assert_eq!(
            drag(&frame, (0, 0), (200, 200)).1.as_deref(),
            Some("first\nlast")
        );
        assert!(drag(&frame, (200, 0), (0, 0)).1.is_none());
    }

    #[test]
    fn click_unmatched_reports_and_cancel_do_not_copy() {
        let frame = frame(&["text"]);
        assert!(drag(&frame, (0, 0), (0, 0)).1.is_none());
        for cancel in [PointerEvent::FocusLost, PointerEvent::Wheel { up: true }] {
            let mut selection = MouseSelection::default();
            selection.handle(PointerEvent::PrimaryPress { column: 0, row: 0 }, &frame);
            selection.handle(cancel, &frame);
            selection.handle(PointerEvent::PrimaryDrag { column: 3, row: 0 }, &frame);
            assert!(selection
                .handle(PointerEvent::PrimaryRelease { column: 3, row: 0 }, &frame)
                .copy
                .is_none());
            assert!(!selection.is_dragging());
        }
    }

    #[test]
    fn only_committed_compatible_epochs_can_copy() {
        let first = frame(&["text"]);
        let mut next = first.clone();
        next.set_epoch(2);
        assert!(first.same_geometry(&next));
        next.set_context(3);
        assert!(!first.same_geometry(&next));
        assert!(!first.same_geometry(&SelectionFrame::for_viewport(21, 1)));
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 0, row: 0 }, &first);
        assert!(selection
            .handle(PointerEvent::PrimaryRelease { column: 3, row: 0 }, &next)
            .copy
            .is_none());
        next.set_epoch(0);
        assert!(drag(&next, (0, 0), (3, 0)).1.is_none());
    }
}
