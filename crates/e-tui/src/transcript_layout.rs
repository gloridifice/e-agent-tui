//! Shared width-aware transcript layout primitives.
//!
//! UI rendering and copy navigation use these exact greedy word-wrapping,
//! activity truncation, and provenance row contracts. This module is a leaf:
//! it knows Ratatui lines but not application state, renderers, or copy mode.

use std::collections::HashMap;

use ratatui::text::Line;

use crate::{
    config::Theme,
    display::{CardRole, ContentCard, DisplayId},
    render::{render_markdown, RenderLine, RenderOptions},
};

#[derive(Debug, Clone)]
struct MarkdownLayoutEntry {
    source: String,
    content_width: Option<usize>,
    link_tags: Vec<crate::display::TaggedLink>,
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
            entry.source == source
                && entry.content_width == options.content_width
                && entry.link_tags == options.link_tags
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
                    link_tags: options.link_tags.clone(),
                    unit_start,
                    lines,
                },
            );
        }
        &self.entries.get(id).expect("entry was materialized").lines
    }

    pub(crate) fn materialize_card(
        &mut self,
        card: &ContentCard,
        theme: &Theme,
        options: &RenderOptions,
    ) {
        let unit = card.unit.unwrap_or_default();
        if self.entries.get(&card.id).is_some_and(|entry| {
            entry.source == card.content
                && entry.content_width == options.content_width
                && entry.unit_start == unit
        }) {
            return;
        }
        let mut card_theme = *theme;
        card_theme.markdown.text = if card.role == CardRole::User {
            theme.input.text
        } else {
            theme.card.attachment
        };
        let mut lines = render_markdown(
            &card.content,
            &card_theme,
            &mut 0,
            options,
            &mut HashMap::new(),
        );
        for line in &mut lines {
            line.unit = unit;
            line.raw_line = None;
            line.atomic = true;
        }
        self.entries.insert(
            card.id.clone(),
            MarkdownLayoutEntry {
                source: card.content.clone(),
                content_width: options.content_width,
                link_tags: Vec::new(),
                unit_start: unit,
                lines,
            },
        );
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

pub(crate) fn user_message_geometry(card: &ContentCard, width: usize) -> (usize, usize) {
    let width = width.max(1);
    let padding = card.horizontal_padding.min(width);
    let ruled = card.role == CardRole::User;
    let gutter = padding.saturating_add(usize::from(ruled)).min(width - 1);
    let right_padding = if ruled {
        padding.min(width - gutter - 1)
    } else {
        0
    };
    let content_width = width - gutter - right_padding;
    (gutter, content_width)
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
    fn mermaid_clips_without_wrapping_and_restores_on_resize() {
        let id = DisplayId::correlated("assistant", "mermaid-width");
        let theme = Theme::ferra();
        let source = "```mermaid\ngraph LR\n A[\"first long box\"] --> B[\"中文节点内容\"] --> C[\"third long box\"]\n```";
        let mut registry = MarkdownLayoutRegistry::default();
        let mut next_unit = 0;
        let mut units = HashMap::new();
        let mut options = RenderOptions::default();
        let full = registry
            .materialize(&id, source, &theme, &mut next_unit, &options, &mut units)
            .to_vec();
        let full_width = full.iter().map(|row| row.line.width()).max().unwrap();
        assert!(full_width > 24);

        for width in [24, full_width, 1, 9, full_width - 1, full_width + 8] {
            options.content_width = Some(width);
            let rows =
                registry.materialize(&id, source, &theme, &mut next_unit, &options, &mut units);
            assert_eq!(rows.len(), full.len());
            assert!(rows
                .iter()
                .all(|row| row.atomic && row.unit == full[0].unit));
            assert_eq!(units.get(&full[0].unit).map(String::as_str), Some(source));
            // Header and bottom padding are not diagram geometry.
            for (row, original) in rows[1..rows.len() - 1].iter().zip(&full[1..full.len() - 1]) {
                assert!(row.line.width() <= width, "width={width}: {:?}", row.line);
                assert_eq!(wrapped_rows(&row.line, width), 1);
                assert_eq!(wrap_line(row.line.clone(), width).len(), 1);
                assert_eq!(row.raw_line, original.raw_line);
                let text = row.line.to_string();
                let original_text = original.line.to_string();
                if original.line.width() > width {
                    let prefix = text.strip_suffix('…').expect("overflow is marked");
                    assert!(original_text.starts_with(prefix));
                } else {
                    assert_eq!(text, original_text);
                }
            }
        }
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
