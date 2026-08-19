//! Transcript rendering cache, isolated from session/event projection.

use std::collections::BTreeSet;

use ratatui::text::Line;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageLineRange {
    pub start: usize,
    pub end: usize,
    pub owns_gap: bool,
}

impl MessageLineRange {
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

#[derive(Default)]
pub struct DisplayLayoutCache {
    pub width: usize,
    pub generation: u64,
    pub row_counts: Vec<usize>,
    /// `prefix[i]` is the first display row of base line `i`; the final item
    /// is the total display-row count.
    pub prefix: Vec<usize>,
}

impl DisplayLayoutCache {
    fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn total_rows(&self) -> usize {
        self.prefix.last().copied().unwrap_or(0)
    }

    /// Return `(base line index, wrapped-row offset inside that line)`.
    pub fn locate(&self, display_row: usize) -> (usize, usize) {
        if self.row_counts.is_empty() {
            return (0, 0);
        }
        let row = display_row.min(self.total_rows().saturating_sub(1));
        let base = self.prefix.partition_point(|start| *start <= row);
        let index = base.saturating_sub(1).min(self.row_counts.len() - 1);
        (index, row.saturating_sub(self.prefix[index]))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheWorkStats {
    pub rebuilds: u64,
    pub patches: u64,
    pub materialized_rows: u64,
}

#[derive(Default)]
pub struct TranscriptRenderCache {
    /// Styled, unwrapped base lines. Gaps are represented as empty lines.
    pub lines: Vec<Line<'static>>,
    pub valid: bool,
    pub tail_dirty: bool,
    pub tail_len: usize,
    /// Previous display-row total captured before history prepend.
    pub prepend_anchor: Option<usize>,
    pub width: usize,
    pub generation: u64,
    pub message_ranges: Vec<Option<MessageLineRange>>,
    pub dirty_messages: BTreeSet<usize>,
    pub layout: DisplayLayoutCache,
    pub work: CacheWorkStats,
}

impl TranscriptRenderCache {
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn mark_tail_dirty(&mut self) {
        self.tail_dirty = true;
    }

    pub fn mark_message_dirty(&mut self, index: usize) {
        if self.valid {
            self.dirty_messages.insert(index);
        }
    }

    pub fn invalidate_layout(&mut self) {
        self.layout.clear();
    }

    pub fn structural_rebuilt(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.invalidate_layout();
        self.work.rebuilds += 1;
    }

    /// Record a tail splice while preserving the already-computed layout
    /// prefix. Only `new_row_counts` at and after `start` are replaced; an
    /// absent/stale layout falls back to lazy full recomputation.
    pub fn structural_tail_updated(
        &mut self,
        start: usize,
        width: usize,
        new_row_counts: &[usize],
    ) {
        let previous_generation = self.generation;
        self.generation = self.generation.wrapping_add(1);
        let can_patch = self.layout.width == width
            && self.layout.generation == previous_generation
            && start <= self.layout.row_counts.len()
            && self.layout.prefix.len() == self.layout.row_counts.len() + 1
            && start + new_row_counts.len() == self.lines.len();
        if !can_patch {
            self.invalidate_layout();
            return;
        }

        self.layout.row_counts.truncate(start);
        self.layout.prefix.truncate(start + 1);
        let mut total = self.layout.prefix.last().copied().unwrap_or(0);
        for count in new_row_counts.iter().copied().map(|count| count.max(1)) {
            self.layout.row_counts.push(count);
            total = total.saturating_add(count);
            self.layout.prefix.push(total);
        }
        self.layout.generation = self.generation;
    }

    pub fn patched(&mut self, count: usize) {
        self.work.patches += count as u64;
    }

    pub fn record_materialized_rows(&mut self, count: usize) {
        self.work.materialized_rows += count as u64;
    }

    pub fn take_work_stats(&mut self) -> CacheWorkStats {
        std::mem::take(&mut self.work)
    }

    pub fn ensure_layout<F>(&mut self, width: usize, mut row_count: F)
    where
        F: FnMut(&Line<'static>, usize) -> usize,
    {
        if self.layout.width == width
            && self.layout.generation == self.generation
            && self.layout.row_counts.len() == self.lines.len()
        {
            return;
        }
        let _zone = crate::tracy_zone!("display layout");
        self.layout.width = width;
        self.layout.generation = self.generation;
        self.layout.row_counts.clear();
        self.layout.prefix.clear();
        self.layout.prefix.reserve(self.lines.len() + 1);
        self.layout.prefix.push(0);
        let mut total = 0usize;
        for line in &self.lines {
            let count = row_count(line, width).max(1);
            self.layout.row_counts.push(count);
            total = total.saturating_add(count);
            self.layout.prefix.push(total);
        }
    }

    pub fn display_len(&self) -> usize {
        if self.layout.generation == self.generation
            && self.layout.row_counts.len() == self.lines.len()
        {
            self.layout.total_rows()
        } else {
            self.lines.len()
        }
    }

    pub fn reset(&mut self) {
        *self = Self {
            width: 80,
            ..Self::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_layout_locates_wrapped_rows() {
        let mut cache = TranscriptRenderCache::default();
        cache.lines = vec![Line::from("abcdef"), Line::from("x")];
        cache.generation = 1;
        cache.ensure_layout(3, |line, width| line.width().div_ceil(width));
        assert_eq!(cache.layout.prefix, vec![0, 2, 3]);
        assert_eq!(cache.layout.locate(0), (0, 0));
        assert_eq!(cache.layout.locate(1), (0, 1));
        assert_eq!(cache.layout.locate(2), (1, 0));
    }

    #[test]
    fn layout_reuses_matching_generation_and_width() {
        let mut cache = TranscriptRenderCache::default();
        cache.lines = vec![Line::from("x"), Line::from("y")];
        cache.generation = 1;
        let mut calls = 0;
        cache.ensure_layout(3, |_, _| {
            calls += 1;
            1
        });
        cache.ensure_layout(3, |_, _| {
            calls += 1;
            1
        });
        assert_eq!(calls, 2);
        cache.lines[1] = Line::from("yz");
        cache.structural_tail_updated(1, 3, &[1]);
        cache.ensure_layout(3, |_, _| {
            calls += 1;
            1
        });
        assert_eq!(calls, 2, "tail splice preserves the prefix layout");
        assert_eq!(cache.layout.prefix, vec![0, 1, 2]);
    }
}
