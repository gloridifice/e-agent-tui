//! Shared width-aware transcript layout primitives.
//!
//! UI rendering and copy navigation use these exact grapheme wrapping,
//! activity truncation, and provenance row contracts. This module is a leaf:
//! it knows Ratatui lines but not application state, renderers, or copy mode.

use std::collections::HashMap;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    config::Theme,
    display::DisplayId,
    render::{render_markdown, RenderLine, RenderOptions},
};

#[derive(Debug, Clone)]
struct MarkdownLayoutEntry {
    source: String,
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
        let unchanged = self
            .entries
            .get(id)
            .is_some_and(|entry| entry.source == source);
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

trait WrapSink {
    fn segment(&mut self, text: &str, style: Style);
    fn end_row(&mut self, base: Style);
}

#[derive(Default)]
struct CountWrapSink {
    rows: usize,
}

impl WrapSink for CountWrapSink {
    fn segment(&mut self, _text: &str, _style: Style) {}

    fn end_row(&mut self, _base: Style) {
        self.rows += 1;
    }
}

#[derive(Default)]
struct LineWrapSink {
    current: Vec<Span<'static>>,
    rows: Vec<Line<'static>>,
}

impl WrapSink for LineWrapSink {
    fn segment(&mut self, text: &str, style: Style) {
        if !text.is_empty() {
            self.current.push(Span::styled(text.to_owned(), style));
        }
    }

    fn end_row(&mut self, base: Style) {
        self.rows
            .push(Line::from(std::mem::take(&mut self.current)).patch_style(base));
    }
}

/// One linear grapheme/display-width scan shared by row counting and
/// materializing. Flattening span text keeps combining marks and emoji ZWJ
/// sequences together even when a style boundary bisects one.
fn scan_wrapped<S: WrapSink>(line: &Line<'static>, width: usize, sink: &mut S) {
    let base = line.style;
    let mut text = String::new();
    let mut styles = Vec::with_capacity(line.spans.len());
    for span in &line.spans {
        let start = text.len();
        text.push_str(span.content.as_ref());
        styles.push((start, text.len(), span.style));
    }
    let emit = |start: usize, end: usize, sink: &mut S| {
        for (style_start, style_end, style) in &styles {
            let from = start.max(*style_start);
            let to = end.min(*style_end);
            if from < to {
                sink.segment(&text[from..to], *style);
            }
        }
    };

    let mut used = 0usize;
    let mut have = false;
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if grapheme_width > width {
            if have {
                sink.end_row(base);
            }
            emit(start, end, sink);
            sink.end_row(base);
            used = 0;
            have = false;
            continue;
        }
        if have && used + grapheme_width > width {
            sink.end_row(base);
            used = 0;
        }
        emit(start, end, sink);
        used += grapheme_width;
        have = true;
        if used == width {
            sink.end_row(base);
            used = 0;
            have = false;
        }
    }
    if have {
        sink.end_row(base);
    }
}

/// Split one line into wrapped rows without Ratatui's exact-width phantom row.
pub fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width {
        return vec![line];
    }
    let mut sink = LineWrapSink::default();
    scan_wrapped(&line, width, &mut sink);
    if sink.rows.is_empty() {
        vec![Line::default().patch_style(line.style)]
    } else {
        sink.rows
    }
}

pub fn wrapped_rows(line: &Line<'static>, width: usize) -> usize {
    if width == 0 || line.width() <= width {
        return 1;
    }
    let mut sink = CountWrapSink::default();
    scan_wrapped(line, width, &mut sink);
    sink.rows.max(1)
}

/// Keep an activity on one display row at the resolved page width.
pub fn truncate_activity_line(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    let base = line.style;
    if width == 0 {
        return Line::default().patch_style(base);
    }

    let budget = width - 1;
    let mut used = 0usize;
    let mut spans = Vec::new();
    let mut ellipsis_style = Style::default();
    'outer: for span in line.spans {
        let mut kept = String::new();
        ellipsis_style = span.style;
        for character in span.content.chars() {
            let char_width = UnicodeWidthChar::width(character).unwrap_or(0);
            if used + char_width > budget {
                if !kept.is_empty() {
                    spans.push(Span::styled(kept, span.style));
                }
                break 'outer;
            }
            kept.push(character);
            used += char_width;
        }
        if !kept.is_empty() {
            spans.push(Span::styled(kept, span.style));
        }
    }
    spans.push(Span::styled("…", ellipsis_style));
    Line::from(spans).patch_style(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

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
}
