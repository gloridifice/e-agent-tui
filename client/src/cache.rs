//! Transcript rendering cache, isolated from the session/event projection.
//! The model only invalidates this boundary; ratatui lines and splice/anchor
//! bookkeeping no longer appear as independent fields on `AppState`.

#[derive(Default)]
pub struct TranscriptRenderCache {
    pub lines: Vec<ratatui::text::Line<'static>>,
    pub valid: bool,
    pub tail_dirty: bool,
    pub tail_len: usize,
    pub prepend_anchor: Option<usize>,
    pub width: usize,
}

impl TranscriptRenderCache {
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn mark_tail_dirty(&mut self) {
        self.tail_dirty = true;
    }

    pub fn reset(&mut self) {
        *self = Self {
            width: 80,
            ..Self::default()
        };
    }
}
