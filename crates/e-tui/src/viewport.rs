//! Transient screen allocation and divider interaction for conversations.
use ratatui::layout::Rect;

use crate::event::PointerEvent;

const MIN_CONVERSATION_WIDTH: u16 = 24;

#[derive(Debug, Clone, Copy)]
pub struct ConversationSplit {
    percent: u16,
    drag: Option<ConversationSplitDrag>,
}

#[derive(Debug, Clone, Copy)]
struct ConversationSplitDrag {
    start_column: u16,
    start_width: u16,
    pending_percent: u16,
}

impl Default for ConversationSplit {
    fn default() -> Self {
        Self {
            percent: 5_000,
            drag: None,
        }
    }
}

impl ConversationSplit {
    pub const MIN_WIDTH: u16 = MIN_CONVERSATION_WIDTH * 2 + 1;

    pub fn is_active(self) -> bool {
        self.drag.is_some()
    }

    pub fn cancel(&mut self) {
        self.drag = None;
    }

    pub fn layout(self, area: Rect) -> Option<(Rect, Rect)> {
        if area.width < Self::MIN_WIDTH {
            return None;
        }
        let available = area.width - 1;
        let percent = self.drag.map_or(self.percent, |drag| drag.pending_percent);
        let left = ((u32::from(available) * u32::from(percent) + 5_000) / 10_000) as u16;
        let left = left.clamp(MIN_CONVERSATION_WIDTH, available - MIN_CONVERSATION_WIDTH);
        Some((
            Rect::new(area.x, area.y, left, area.height),
            Rect::new(area.x + left + 1, area.y, available - left, area.height),
        ))
    }

    pub fn handle_pointer(&mut self, pointer: PointerEvent, area: Option<Rect>) -> bool {
        let Some((area, (left, _))) =
            area.and_then(|area| self.layout(area).map(|panes| (area, panes)))
        else {
            let captured = self.is_active();
            self.cancel();
            return captured;
        };
        if self.is_active() {
            match pointer {
                PointerEvent::PrimaryDrag { column, .. } => self.update(column, area.width - 1),
                PointerEvent::PrimaryRelease { column, .. } => {
                    self.update(column, area.width - 1);
                    self.percent = self.drag.take().unwrap().pending_percent;
                }
                PointerEvent::FocusLost => self.cancel(),
                _ => {}
            }
            return true;
        }
        if let PointerEvent::PrimaryPress { column, row } = pointer {
            if area.contains((column, row).into()) && left.right().abs_diff(column) <= 1 {
                self.drag = Some(ConversationSplitDrag {
                    start_column: column,
                    start_width: left.width,
                    pending_percent: self.percent,
                });
                return true;
            }
        }
        false
    }

    fn update(&mut self, column: u16, available: u16) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        if column == drag.start_column {
            drag.pending_percent = self.percent;
            return;
        }
        let width = (i32::from(drag.start_width) + i32::from(column) - i32::from(drag.start_column))
            .clamp(
                i32::from(MIN_CONVERSATION_WIDTH),
                i32::from(available - MIN_CONVERSATION_WIDTH),
            ) as u32;
        drag.pending_percent =
            ((width * 10_000 + u32::from(available) / 2) / u32::from(available)) as u16;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConversationViewport {
    pub main: Option<Rect>,
    pub preview: Option<Rect>,
    pub focused: bool,
    pub resize_percent: Option<u16>,
}

impl ConversationViewport {
    pub fn contains(self, x: u16, y: u16) -> bool {
        self.main
            .into_iter()
            .chain(self.preview)
            .any(|area| area.contains((x, y).into()))
    }
}
