//! Ratatui adapter for selectable rendered rows and final-layer highlighting.

use ratatui::{buffer::Buffer, style::Modifier, text::Line};

use crate::mouse_selection::{MouseSelection, SelectionFrame, SelectionSurface};

pub(crate) fn register_line(
    frame: &mut SelectionFrame,
    surface: SelectionSurface,
    order: usize,
    x: u16,
    y: u16,
    line: &Line<'_>,
) {
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    frame.push_text(surface, order, x, y, &text);
}

pub(crate) fn paint(frame: &SelectionFrame, selection: &MouseSelection, buffer: &mut Buffer) {
    for range in frame.selected_cell_ranges(selection) {
        for x in range.x..range.x.saturating_add(range.width) {
            if x >= buffer.area.x
                && x < buffer.area.x.saturating_add(buffer.area.width)
                && range.y >= buffer.area.y
                && range.y < buffer.area.y.saturating_add(buffer.area.height)
            {
                let cell = &mut buffer[(x, range.y)];
                cell.set_style(cell.style().add_modifier(Modifier::REVERSED));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect, style::Color, text::Line};

    use super::*;
    use crate::event::PointerEvent;

    #[test]
    fn paint_preserves_existing_color_and_marks_wide_cells() {
        let mut frame = SelectionFrame::for_viewport(8, 1);
        frame.set_epoch(1);
        register_line(
            &mut frame,
            SelectionSurface::Transcript,
            0,
            0,
            0,
            &Line::from("A界B"),
        );
        let mut selection = MouseSelection::default();
        selection.handle(PointerEvent::PrimaryPress { column: 0, row: 0 }, &frame);
        selection.handle(PointerEvent::PrimaryDrag { column: 1, row: 0 }, &frame);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer[(1, 0)].set_fg(Color::Red);

        paint(&frame, &selection, &mut buffer);

        assert_eq!(buffer[(1, 0)].fg, Color::Red);
        for x in 1..=2 {
            assert!(buffer[(x, 0)].modifier.contains(Modifier::REVERSED));
        }
    }
}
