//! Final-buffer adaptation, committed presentation, and selection highlighting.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

use ratatui::{
    buffer::{Buffer, CellWidth},
    layout::Position,
    style::Modifier,
};

use crate::{
    app::TuiApp,
    input_page::InputPageSession,
    interaction::ApprovalCard,
    mouse_selection::{MouseSelection, SelectionFrame},
};

pub fn selection_context(
    state: &TuiApp,
    page: Option<&InputPageSession>,
    approval: Option<&ApprovalCard>,
    help_visible: bool,
) -> u64 {
    let mut hash = DefaultHasher::new();
    state.session.session_id.hash(&mut hash);
    state.session.new_conversation.is_some().hash(&mut hash);
    page.map(|page| std::mem::discriminant(&page.page))
        .hash(&mut hash);
    page.and_then(InputPageSession::question_rpc_id)
        .hash(&mut hash);
    approval.map(|approval| &approval.id).hash(&mut hash);
    help_visible.hash(&mut hash);
    state.reading.is_some().hash(&mut hash);
    state.preview.fullscreen.hash(&mut hash);
    hash.finish()
}

/// Runner-owned presentation. Clones share immutable screen-sized artifacts.
#[derive(Clone, Default)]
pub struct Presentation {
    buffer: Option<Arc<Buffer>>,
    selection_frame: SelectionFrame,
    cursor: Option<Position>,
}

impl Presentation {
    pub fn selection_frame(&self) -> &SelectionFrame {
        &self.selection_frame
    }

    pub(crate) fn capture(
        buffer: &Buffer,
        cursor: Option<Position>,
        context: u64,
        selectable: bool,
    ) -> Self {
        let mut selection_frame = if selectable {
            collect(buffer)
        } else {
            SelectionFrame::default()
        };
        selection_frame.set_context(context);
        Self {
            buffer: Some(Arc::new(buffer.clone())),
            selection_frame,
            cursor,
        }
    }

    pub(crate) fn replay(
        &self,
        buffer: &mut Buffer,
        selection: &MouseSelection,
        context: u64,
    ) -> Option<Option<Position>> {
        let saved = self.buffer.as_ref()?;
        if !selection.holds(&self.selection_frame)
            || context != self.selection_frame.context()
            || saved.area != buffer.area
        {
            return None;
        }
        buffer.clone_from(saved);
        paint(&self.selection_frame, selection, buffer);
        Some(self.cursor)
    }

    /// Call only after the complete terminal transaction has succeeded.
    pub fn commit(&mut self, mut candidate: Self, selection: &mut MouseSelection) {
        let changed = !candidate
            .selection_frame
            .same_geometry(&self.selection_frame);
        let epoch = if changed {
            self.selection_frame.epoch().wrapping_add(1).max(1)
        } else {
            self.selection_frame.epoch().max(1)
        };
        candidate.selection_frame.set_epoch(epoch);
        *self = candidate;
        if changed {
            selection.clear();
        }
    }
}

fn collect(buffer: &Buffer) -> SelectionFrame {
    let area = buffer.area;
    let mut frame = SelectionFrame::for_viewport(area.right(), area.bottom());
    for y in area.y..area.bottom() {
        let mut x = area.x;
        while x < area.right() {
            let cell = &buffer[(x, y)];
            let width = cell.cell_width().max(1).min(area.right() - x);
            if !cell.modifier.contains(Modifier::HIDDEN) {
                frame.put_glyph(x, y, cell.symbol(), width);
            }
            x += width;
        }
    }
    frame
}

pub(crate) fn paint(frame: &SelectionFrame, selection: &MouseSelection, buffer: &mut Buffer) {
    for range in frame.selected_cell_ranges(selection) {
        for x in range.x..range.x.saturating_add(range.width) {
            if let Some(cell) = buffer.cell_mut((x, range.y)) {
                // Toggle relative to the saved style, including software cursors.
                cell.modifier.toggle(Modifier::REVERSED);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::PointerEvent;
    use ratatui::{
        layout::Rect,
        style::{Color, Style},
        widgets::{Clear, Paragraph, Widget},
    };

    fn select(
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
        let copy = selection
            .handle(
                PointerEvent::PrimaryRelease {
                    column: end.0,
                    row: end.1,
                },
                frame,
            )
            .copy;
        (selection, copy)
    }

    #[test]
    fn final_cells_respect_wide_glyphs_clipping_and_overlay_overwrite() {
        let area = Rect::new(0, 0, 12, 2);
        let mut buffer = Buffer::empty(area);
        Paragraph::new("A界e\u{301}👩‍💻").render(Rect::new(0, 0, 6, 1), &mut buffer);
        Paragraph::new("secret below").render(Rect::new(0, 1, 12, 1), &mut buffer);
        Clear.render(Rect::new(0, 1, 12, 1), &mut buffer);
        Paragraph::new("●● popup…").render(Rect::new(0, 1, 12, 1), &mut buffer);
        Paragraph::new("界").render(Rect::new(11, 0, 1, 1), &mut buffer);
        let mut frame = collect(&buffer);
        frame.set_epoch(1);
        assert_eq!(
            select(&frame, (0, 0), (11, 1)).1.as_deref(),
            Some("A界e\u{301}👩‍💻\n●● popup…")
        );
        assert_eq!(select(&frame, (1, 0), (2, 0)).1.as_deref(), Some("界"));
    }

    #[test]
    fn concealment_never_exports_the_buffer_symbol() {
        let mut buffer = Buffer::with_lines(["visible secret"]);
        buffer.set_style(
            Rect::new(8, 0, 6, 1),
            Style::new().add_modifier(Modifier::HIDDEN),
        );
        let mut frame = collect(&buffer);
        frame.set_epoch(1);
        assert_eq!(
            select(&frame, (0, 0), (13, 0)).1.as_deref(),
            Some("visible")
        );
    }

    #[test]
    fn paint_preserves_colors_and_distinguishes_reversed_wide_cells() {
        let mut buffer = Buffer::with_lines(["A界B"]);
        buffer[(1, 0)].set_fg(Color::Red);
        buffer[(1, 0)].modifier = Modifier::REVERSED;
        let base = buffer.clone();
        let mut frame = collect(&buffer);
        frame.set_epoch(1);
        let (selection, _) = select(&frame, (0, 0), (2, 0));
        paint(&frame, &selection, &mut buffer);
        assert_eq!(buffer[(1, 0)].fg, Color::Red);
        assert!(!buffer[(1, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buffer[(2, 0)].modifier.contains(Modifier::REVERSED));
        assert!(base[(1, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn held_snapshot_and_commit_are_independent_of_candidates() {
        let mut committed = Presentation::default();
        let mut selection = MouseSelection::default();
        let base = Buffer::with_lines(["old screen"]);
        committed.commit(
            Presentation::capture(&base, Some(Position::new(2, 0)), 7, true),
            &mut selection,
        );
        selection.handle(
            PointerEvent::PrimaryPress { column: 0, row: 0 },
            committed.selection_frame(),
        );
        selection.handle(
            PointerEvent::PrimaryDrag { column: 2, row: 0 },
            committed.selection_frame(),
        );
        let failed_candidate =
            Presentation::capture(&Buffer::with_lines(["new screen"]), None, 7, true);
        drop(failed_candidate);
        let mut output = Buffer::empty(base.area);
        assert_eq!(
            committed.replay(&mut output, &selection, 7),
            Some(Some(Position::new(2, 0)))
        );
        assert_eq!(output[(0, 0)].symbol(), "o");
        assert_eq!(committed.buffer.as_ref().unwrap().as_ref(), &base);
        assert!(committed.replay(&mut output, &selection, 8).is_none());
        assert_eq!(
            selection
                .handle(
                    PointerEvent::PrimaryRelease { column: 2, row: 0 },
                    committed.selection_frame()
                )
                .copy
                .as_deref(),
            Some("old")
        );
        committed.commit(Presentation::capture(&base, None, 8, true), &mut selection);
        assert_eq!(selection, MouseSelection::default());
    }
}
