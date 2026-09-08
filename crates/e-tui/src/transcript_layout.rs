//! Shared width-aware transcript layout primitives.
//!
//! UI rendering and copy navigation use these exact greedy word-wrapping,
//! activity truncation, and provenance row contracts. This module is a leaf:
//! it knows Ratatui lines but not application state, renderers, or copy mode.

use std::collections::HashMap;

use ratatui::text::Line;

use crate::{
    config::Theme,
    display::DisplayId,
    render::{render_markdown, RenderLine, RenderOptions},
};

#[derive(Debug, Clone)]
struct MarkdownLayoutEntry {
    source: String,
    content_width: Option<usize>,
    unit_start: u64,
    lines: Vec<RenderLine>,
}

/// Render-only Markdown state keyed by stable display identity. Transcript
/// storage remains source-only; this registry owns styled lines and the full
/// copy-provenance unit range.
#[derive(Debug, Default)]
pub struct MarkdownLayoutRegistry {
    entries: HashMap<DisplayId, MarkdownLayoutEntry>,
}

impl MarkdownLayoutRegistry {
    pub fn materialize(
        &mut self,
        id: &DisplayId,
        source: &str,
        theme: &Theme,
        next_unit: &mut u64,
        options: &RenderOptions,
        units: &mut HashMap<u64, String>,
    ) -> &[RenderLine] {
        let unchanged = self.entries.get(id).is_some_and(|entry| {
            entry.source == source && entry.content_width == options.content_width
        });
        if !unchanged {
            let unit_start = self
                .entries
                .get(id)
                .map_or(*next_unit, |entry| entry.unit_start);
            let mut local_next = unit_start;
            let lines = render_markdown(source, theme, &mut local_next, options, units);
            *next_unit = (*next_unit).max(local_next);
            self.entries.insert(
                id.clone(),
                MarkdownLayoutEntry {
                    source: source.to_owned(),
                    content_width: options.content_width,
                    unit_start,
                    lines,
                },
            );
        }
        &self.entries.get(id).expect("entry was materialized").lines
    }

    pub fn lines(&self, id: &DisplayId) -> Option<&[RenderLine]> {
        self.entries.get(id).map(|entry| entry.lines.as_slice())
    }

    pub fn unit_start(&self, id: &DisplayId) -> Option<u64> {
        self.entries.get(id).map(|entry| entry.unit_start)
    }

    pub fn display_for_unit(&self, unit: u64) -> Option<&DisplayId> {
        self.entries.iter().find_map(|(id, entry)| {
            entry
                .lines
                .iter()
                .any(|line| line.unit == unit)
                .then_some(id)
        })
    }

    pub fn invalidate(&mut self, id: &DisplayId) {
        if let Some(entry) = self.entries.get_mut(id) {
            // Retain the stable range start while forcing rematerialization.
            entry.source.clear();
        }
    }

    pub fn invalidate_all(&mut self) {
        for entry in self.entries.values_mut() {
            entry.source.clear();
        }
    }

    pub fn remove(&mut self, id: &DisplayId) {
        self.entries.remove(id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceLayoutRow {
    pub unit: u64,
    pub raw_line: Option<usize>,
    pub atomic: bool,
    pub text: String,
    pub global_row: usize,
}

/// Width-aware greedy word wrapping shared with preview and the input bar.
pub use crate::wrap::{
    clip_line, clip_text, ellipsize_line, ellipsize_text, wrap_line, wrapped_rows,
};

/// Keep an activity on one display row at the resolved page width.
pub fn truncate_activity_line(line: Line<'static>, width: usize) -> Line<'static> {
    crate::wrap::ellipsize_line(line, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        style::{Color, Style},
        text::Span,
    };

    #[test]
    fn markdown_registry_reuses_display_unit_range_when_source_grows() {
        let id = DisplayId::correlated("assistant", "turn-1");
        let theme = crate::config::Config::default().theme();
        let mut registry = MarkdownLayoutRegistry::default();
        let mut next_unit = 0;
        let mut units = HashMap::new();
        let options = RenderOptions::default();
        registry.materialize(&id, "first", &theme, &mut next_unit, &options, &mut units);
        let start = registry.unit_start(&id).unwrap();
        registry.materialize(
            &id,
            "first\n\nsecond",
            &theme,
            &mut next_unit,
            &options,
            &mut units,
        );
        assert_eq!(registry.unit_start(&id), Some(start));
        assert_eq!(registry.lines(&id).unwrap()[0].unit, start);
    }

    #[test]
    fn markdown_registry_rerenders_when_content_width_changes() {
        let id = DisplayId::correlated("assistant", "width-change");
        let theme = crate::config::Config::default().theme();
        let mut registry = MarkdownLayoutRegistry::default();
        let mut next_unit = 0;
        let mut units = HashMap::new();
        let mut options = RenderOptions::default();
        options.content_width = Some(24);
        let source = "| c |\n|---|\n| xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx |";
        registry.materialize(&id, source, &theme, &mut next_unit, &options, &mut units);
        let narrow_rows = registry.lines(&id).unwrap().len();
        options.content_width = Some(64);
        registry.materialize(&id, source, &theme, &mut next_unit, &options, &mut units);
        let wide_rows = registry.lines(&id).unwrap().len();
        assert!(
            narrow_rows > wide_rows,
            "narrower table should wrap into more rows: narrow={narrow_rows} wide={wide_rows}"
        );
    }

    #[test]
    fn wrapping_keeps_combining_and_zwj_graphemes_intact_across_styles() {
        let line = Line::from(vec![
            Span::styled("e", Style::default().fg(Color::Red)),
            Span::styled("\u{301}👩", Style::default().fg(Color::Blue)),
            Span::styled("\u{200d}💻x", Style::default().fg(Color::Green)),
        ]);
        let rows = wrap_line(line, 2);
        let text: Vec<String> = rows
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        assert_eq!(text, vec!["e\u{301}", "👩\u{200d}💻", "x"]);
    }

    #[test]
    fn row_count_matches_materialized_wraps() {
        for width in 1..=8 {
            let line = Line::from("a中bcdef");
            assert_eq!(wrapped_rows(&line, width), wrap_line(line, width).len());
        }
    }

    #[test]
    fn activity_truncation_reserves_the_ellipsis_column() {
        let line = truncate_activity_line(Line::from("abcdef"), 4);
        assert_eq!(line.to_string(), "abc…");
        assert_eq!(line.width(), 4);
    }
    #[test]
    fn clipping_is_exact_fit_and_grapheme_safe_across_styles() {
        use ratatui::style::Color;

        assert_eq!(ellipsize_text("abc", 3), "abc");
        assert_eq!(ellipsize_text("abcd", 3), "ab…");
        let line = Line::from(vec![
            Span::styled("e", Style::default().fg(Color::Red)),
            Span::styled("\u{301}👩", Style::default().fg(Color::Blue)),
            Span::styled("\u{200d}💻x", Style::default().fg(Color::Green)),
        ]);
        assert_eq!(clip_line(line.clone(), 1).to_string(), "e\u{301}");
        assert_eq!(ellipsize_line(line, 2).to_string(), "e\u{301}…");
    }
}
