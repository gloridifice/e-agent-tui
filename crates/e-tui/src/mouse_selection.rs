//! Application-owned mouse selection over the last committed visible frame.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::event::PointerEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionSurface {
    Transcript,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Point {
    surface: SelectionSurface,
    row: usize,
    grapheme: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectableRow {
    surface: SelectionSurface,
    order: usize,
    x: u16,
    y: u16,
    graphemes: Vec<String>,
    widths: Vec<usize>,
}

impl SelectableRow {
    fn from_text(surface: SelectionSurface, order: usize, x: u16, y: u16, text: &str) -> Self {
        let graphemes = text.graphemes(true).map(str::to_owned).collect::<Vec<_>>();
        let widths = graphemes
            .iter()
            .map(|grapheme| UnicodeWidthStr::width(grapheme.as_str()))
            .collect();
        Self {
            surface,
            order,
            x,
            y,
            graphemes,
            widths,
        }
    }

    fn width(&self) -> usize {
        self.widths.iter().sum()
    }

    fn point_at(&self, column: u16) -> Option<Point> {
        if self.graphemes.is_empty() || column < self.x {
            return None;
        }
        let local = usize::from(column - self.x);
        if local >= self.width() {
            return None;
        }
        let mut start = 0;
        for (grapheme, width) in self.widths.iter().copied().enumerate() {
            if local < start + width.max(1) {
                return Some(Point {
                    surface: self.surface,
                    row: self.order,
                    grapheme,
                });
            }
            start += width;
        }
        None
    }

    fn clamp_point(&self, column: u16) -> Option<Point> {
        if column < self.x {
            return (!self.graphemes.is_empty()).then_some(Point {
                surface: self.surface,
                row: self.order,
                grapheme: 0,
            });
        }
        self.point_at(column).or_else(|| {
            self.graphemes.last().map(|_| Point {
                surface: self.surface,
                row: self.order,
                grapheme: self.graphemes.len().saturating_sub(1),
            })
        })
    }
}

/// Bounded geometry of selectable rows from one successfully committed frame.
#[derive(Debug, Clone, Default)]
pub struct SelectionFrame {
    epoch: u64,
    viewport: (u16, u16),
    rows: Vec<SelectableRow>,
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
            ..Self::default()
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn set_epoch(&mut self, epoch: u64) {
        self.epoch = epoch;
    }

    /// Compare only committed hit-test geometry and text; epoch is assigned by
    /// the runner after this comparison.
    pub fn same_geometry(&self, other: &Self) -> bool {
        self.viewport == other.viewport && self.rows == other.rows
    }

    pub fn matches_viewport(&self, width: u16, height: u16) -> bool {
        self.viewport == (width, height)
    }

    pub fn push_text(
        &mut self,
        surface: SelectionSurface,
        order: usize,
        x: u16,
        y: u16,
        text: &str,
    ) {
        self.rows
            .push(SelectableRow::from_text(surface, order, x, y, text));
    }

    fn point_at(&self, column: u16, row: u16) -> Option<Point> {
        self.rows
            .iter()
            .filter(|entry| entry.y == row)
            .find_map(|entry| entry.point_at(column))
    }

    fn clamped_point(&self, surface: SelectionSurface, column: u16, row: u16) -> Option<Point> {
        self.rows
            .iter()
            .filter(|entry| entry.surface == surface && !entry.graphemes.is_empty())
            .min_by_key(|entry| entry.y.abs_diff(row))
            .and_then(|entry| entry.clamp_point(column))
    }

    fn row(&self, surface: SelectionSurface, order: usize) -> Option<&SelectableRow> {
        self.rows
            .iter()
            .find(|entry| entry.surface == surface && entry.order == order)
    }

    fn bounds(&self, anchor: Point, focus: Point) -> Option<(Point, Point)> {
        if anchor.surface != focus.surface
            || (anchor.row == focus.row && anchor.grapheme == focus.grapheme)
        {
            return None;
        }
        Some(
            if (anchor.row, anchor.grapheme) <= (focus.row, focus.grapheme) {
                (anchor, focus)
            } else {
                (focus, anchor)
            },
        )
    }

    fn extract(&self, anchor: Point, focus: Point) -> Option<String> {
        let (start, end) = self.bounds(anchor, focus)?;
        let mut lines = Vec::new();
        for order in start.row..=end.row {
            let row = self.row(start.surface, order)?;
            let first = if order == start.row {
                start.grapheme
            } else {
                0
            };
            if row.graphemes.is_empty() {
                lines.push(String::new());
                continue;
            }
            let last = if order == end.row {
                end.grapheme
            } else {
                row.graphemes.len() - 1
            };
            lines.push(row.graphemes[first..=last].concat());
        }
        let text = lines.join("\n");
        (!text.is_empty()).then_some(text)
    }

    pub(crate) fn selected_cell_ranges(
        &self,
        selection: &MouseSelection,
    ) -> Vec<SelectionCellRange> {
        let Some((start, end)) = selection.bounds_for(self) else {
            return Vec::new();
        };
        let mut ranges = Vec::with_capacity(end.row.saturating_sub(start.row) + 1);
        for order in start.row..=end.row {
            let Some(row) = self.row(start.surface, order) else {
                continue;
            };
            if row.graphemes.is_empty() {
                continue;
            }
            let first = if order == start.row {
                start.grapheme
            } else {
                0
            };
            let last = if order == end.row {
                end.grapheme
            } else {
                row.graphemes.len() - 1
            };
            let x = row.x.saturating_add(
                row.widths[..first]
                    .iter()
                    .sum::<usize>()
                    .try_into()
                    .unwrap_or(u16::MAX),
            );
            let width = row.widths[first..=last]
                .iter()
                .map(|width| (*width).max(1))
                .sum::<usize>()
                .try_into()
                .unwrap_or(u16::MAX);
            if width > 0 {
                ranges.push(SelectionCellRange { x, y: row.y, width });
            }
        }
        ranges
    }
}

/// Interaction-owned state; the frame remains render-owned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MouseSelection {
    epoch: Option<u64>,
    anchor: Option<Point>,
    focus: Option<Point>,
    dragging: bool,
}

impl MouseSelection {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn handle(&mut self, event: PointerEvent, frame: &SelectionFrame) -> SelectionUpdate {
        let before = self.clone();
        let mut copy = None;
        if self.epoch.is_some_and(|epoch| epoch != frame.epoch()) {
            self.clear();
        }
        match event {
            PointerEvent::Wheel { up: _ } => {}
            PointerEvent::FocusLost => self.clear(),
            PointerEvent::PrimaryPress { column, row } => {
                self.clear();
                if let Some(point) = frame.point_at(column, row) {
                    self.epoch = Some(frame.epoch());
                    self.anchor = Some(point);
                    self.focus = Some(point);
                    self.dragging = true;
                }
            }
            PointerEvent::PrimaryDrag { column, row } => {
                if let Some(anchor) = self.anchor {
                    self.focus = frame.clamped_point(anchor.surface, column, row);
                }
            }
            PointerEvent::PrimaryRelease { column, row } => {
                if self.dragging {
                    self.dragging = false;
                    if let Some(anchor) = self.anchor {
                        self.focus = frame.clamped_point(anchor.surface, column, row);
                    }
                    copy = self
                        .bounds_for(frame)
                        .and_then(|(anchor, focus)| frame.extract(anchor, focus));
                }
            }
        }
        SelectionUpdate {
            changed: *self != before,
            copy,
        }
    }

    fn bounds_for(&self, frame: &SelectionFrame) -> Option<(Point, Point)> {
        (self.epoch == Some(frame.epoch()))
            .then_some((self.anchor?, self.focus?))
            .and_then(|(anchor, focus)| frame.bounds(anchor, focus))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(lines: &[&str]) -> SelectionFrame {
        let mut frame = SelectionFrame::for_viewport(80, 24);
        frame.set_epoch(1);
        for (row, line) in lines.iter().enumerate() {
            frame.push_text(SelectionSurface::Transcript, row, 0, row as u16, line);
        }
        frame
    }

    fn copy(
        selection: &mut MouseSelection,
        event: PointerEvent,
        frame: &SelectionFrame,
    ) -> Option<String> {
        selection.handle(event, frame).copy
    }

    #[test]
    fn extracts_backward_multiline_visual_range() {
        let frame = frame(&["alpha", "beta"]);
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 3, row: 1 }, &frame);
        let copied = copy(
            &mut selection,
            PointerEvent::PrimaryRelease { column: 1, row: 0 },
            &frame,
        );
        assert_eq!(copied.as_deref(), Some("lpha\nbeta"));
    }

    #[test]
    fn keeps_wide_and_combining_graphemes_whole() {
        let frame = frame(&["A界🙂éZ"]);
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 1, row: 0 }, &frame);
        let copied = copy(
            &mut selection,
            PointerEvent::PrimaryRelease { column: 4, row: 0 },
            &frame,
        );
        assert_eq!(copied.as_deref(), Some("界🙂"));
        assert_eq!(frame.selected_cell_ranges(&selection)[0].width, 4);
    }

    #[test]
    fn preserves_intentional_blank_rows_and_trailing_spaces() {
        let frame = frame(&["a ", "", "b"]);
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 0, row: 0 }, &frame);
        let copied = copy(
            &mut selection,
            PointerEvent::PrimaryRelease { column: 0, row: 2 },
            &frame,
        );
        assert_eq!(copied.as_deref(), Some("a \n\nb"));
    }

    #[test]
    fn hit_testing_checks_both_split_surfaces_on_the_same_screen_row() {
        let mut frame = SelectionFrame::for_viewport(80, 24);
        frame.set_epoch(1);
        frame.push_text(SelectionSurface::Transcript, 0, 0, 0, "main");
        frame.push_text(SelectionSurface::Preview, 0, 10, 0, "preview");
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 10, row: 0 }, &frame);
        let copied = copy(
            &mut selection,
            PointerEvent::PrimaryRelease { column: 12, row: 0 },
            &frame,
        );
        assert_eq!(copied.as_deref(), Some("pre"));
    }

    #[test]
    fn geometry_comparison_includes_viewport_but_ignores_epoch() {
        let first = frame(&["alpha"]);
        let mut same = first.clone();
        same.set_epoch(9);
        assert!(first.same_geometry(&same));
        assert!(!first.same_geometry(&frame(&["beta"])));
        assert!(!first.same_geometry(&SelectionFrame::for_viewport(81, 24)));

        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 0, row: 0 }, &first);
        let update = selection.handle(PointerEvent::PrimaryRelease { column: 2, row: 0 }, &same);
        assert!(update.copy.is_none());
        assert!(update.changed);
    }

    #[test]
    fn press_rejects_left_padding_but_drag_clamps_to_the_left_edge() {
        let mut frame = SelectionFrame::for_viewport(80, 24);
        frame.set_epoch(1);
        frame.push_text(SelectionSurface::Transcript, 0, 4, 0, "alpha");
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 3, row: 0 }, &frame);
        assert!(copy(
            &mut selection,
            PointerEvent::PrimaryRelease { column: 6, row: 0 },
            &frame,
        )
        .is_none());

        selection.handle(PointerEvent::PrimaryPress { column: 6, row: 0 }, &frame);
        let copied = copy(
            &mut selection,
            PointerEvent::PrimaryRelease { column: 3, row: 0 },
            &frame,
        );
        assert_eq!(copied.as_deref(), Some("alp"));
    }

    #[test]
    fn click_does_not_copy() {
        let frame = frame(&["alpha"]);
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 1, row: 0 }, &frame);
        assert!(copy(
            &mut selection,
            PointerEvent::PrimaryRelease { column: 1, row: 0 },
            &frame,
        )
        .is_none());
    }
}
