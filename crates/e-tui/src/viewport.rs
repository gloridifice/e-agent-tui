//! Transient screen allocation for an independently owned conversation.
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConversationViewport {
    pub main: Option<Rect>,
    pub preview: Option<Rect>,
    pub focused: bool,
}

impl ConversationViewport {
    pub fn contains(self, x: u16, y: u16) -> bool {
        self.main
            .into_iter()
            .chain(self.preview)
            .any(|area| area.contains((x, y).into()))
    }
}
