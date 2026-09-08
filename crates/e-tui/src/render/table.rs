//! Markdown table layout and atomic rendering.

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    config::Theme,
    i18n::tr_args,
    render::{RenderLine, RenderOptions},
};

use super::{collect_inlines, MAX_CELL_WIDTH};

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

pub(super) fn render_table(
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    // Parse cells line-by-line from the raw markdown (keeps the source map
    // exact and avoids another pulldown pass).
    let rows: Vec<Vec<String>> = raw
        .lines()
        .filter(|l| !l.trim().starts_with("|:")) // alignment row?
        .filter_map(|line| {
            let t = line.trim();
            if !t.starts_with('|') {
                return None;
            }
            let mut cells: Vec<String> = t.split('|').map(|c| c.trim().to_string()).collect();
            if !cells.is_empty() && cells[0].is_empty() {
                cells.remove(0);
            }
            if !cells.is_empty() && cells.last().map_or(false, |c| c.is_empty()) {
                cells.pop();
            }
            if cells.is_empty() {
                return None;
            }
            Some(cells)
        })
        .collect();
    if rows.is_empty() {
        // Fall back: plain lines (should not happen for a real table block).
        for (i, line) in raw.lines().enumerate() {
            out.push(RenderLine {
                line: Line::from(Span::styled(line.to_string(), theme.markdown.text.style())),
                unit,
                raw_line: Some(i),
                atomic: false,
                fill: false,
            });
        }
        return;
    }

    let cols = rows.iter().map(Vec::len).max().unwrap_or(1);
    let has_header = rows.len() >= 2
        && rows[1]
            .iter()
            .all(|c| c.chars().all(|ch| ch == '-' || ch == ':'));

    let mut widths: Vec<usize> = vec![1; cols];
    let mut min_widths: Vec<usize> = vec![1; cols];
    let mut layouts: Vec<Vec<TableCellLayout>> = Vec::with_capacity(rows.len());
    for (r, row) in rows.iter().enumerate() {
        let base = if has_header && r == 0 {
            theme.markdown.table_header.style()
        } else {
            theme.markdown.text.style()
        };
        let mut row_layouts = Vec::with_capacity(cols);
        for c in 0..cols {
            let cell = row.get(c).map(String::as_str).unwrap_or("");
            let layout = TableCellLayout::new(theme, cell, base);
            widths[c] = widths[c].max(layout.width);
            min_widths[c] = min_widths[c].max(layout.min_width);
            row_layouts.push(layout);
        }
        layouts.push(row_layouts);
    }
    if options.content_width.is_none() {
        for width in &mut widths {
            *width = (*width).min(MAX_CELL_WIDTH);
        }
    }
    let widths = match options.content_width {
        Some(limit) => fit_table_widths(&widths, &min_widths, limit),
        None => widths,
    };

    let dim_style = theme.markdown.table_border.style();
    let body_rows: Vec<usize> = (0..rows.len())
        .filter(|r| !(*r == 1 && has_header))
        .collect();
    let collapsed =
        !options.expanded.contains(&unit) && body_rows.len() > options.collapse_rows / 2;
    let push_row = |out: &mut Vec<RenderLine>, row_idx: usize, header: bool| {
        if options.content_width.is_some() {
            push_table_cell_rows(out, unit, dim_style, &widths, &layouts[row_idx]);
        } else {
            push_table_cells(out, unit, dim_style, &widths, &rows[row_idx], header, theme);
        }
    };

    push_plain(out, unit, table_border("┌", "┬", "┐", &widths), dim_style);
    if collapsed {
        // Header + separator + first rows … last rows.
        push_row(out, 0, true);
        push_plain(out, unit, table_border("├", "┼", "┤", &widths), dim_style);
        let shown = 3usize.min(body_rows.len().saturating_sub(1));
        for idx in &body_rows[1..1 + shown] {
            push_row(out, *idx, false);
        }
        let hidden = body_rows.len() - shown - 1 - 2;
        let hint_content = tr_args(
            options.language,
            "markdown.table_collapse_hint",
            &[("hidden", hidden.to_string())],
        );
        let total_width = 3 * widths.len() + 1 + widths.iter().sum::<usize>();
        let hint_pad =
            total_width.saturating_sub(UnicodeWidthStr::width(hint_content.as_str()) + 2);
        let hint = format!("│{hint_content}{}│", " ".repeat(hint_pad));
        push_plain(out, unit, hint, dim_style);
        for idx in &body_rows[body_rows.len() - 2..] {
            push_row(out, *idx, false);
        }
    } else {
        for (r, _row) in rows.iter().enumerate() {
            if r == 1 && has_header {
                push_plain(out, unit, table_border("├", "┼", "┤", &widths), dim_style);
                continue;
            }
            push_row(out, r, r == 0);
        }
    }
    push_plain(out, unit, table_border("└", "┴", "┘", &widths), dim_style);
}

/// One single-span styled row (table borders, hints).
fn push_plain(out: &mut Vec<RenderLine>, unit: u64, s: String, style: Style) {
    out.push(RenderLine {
        line: Line::from(Span::styled(s, style)),
        unit,
        raw_line: None,
        atomic: true,
        fill: false,
    });
}

/// One table body row: `│` bars dim, header cells bold (glamour keeps the
/// ┼│─ separator set and bolds the header). Cell content goes through the
/// inline renderer, so `**bold**` / `code` / links inside cells render
/// styled instead of leaking their markdown markers.
fn push_table_cells(
    out: &mut Vec<RenderLine>,
    unit: u64,
    dim_style: Style,
    widths: &[usize],
    cells: &[String],
    header: bool,
    theme: &Theme,
) {
    let cell_base = if header {
        theme.markdown.table_header.style()
    } else {
        theme.markdown.text.style()
    };
    let mut spans = vec![Span::styled("│", dim_style)];
    for (i, w) in widths.iter().enumerate() {
        let cell = cells.get(i).map(String::as_str).unwrap_or("");
        spans.push(Span::styled(" ", dim_style));
        spans.extend(cell_spans(theme, cell, cell_base, *w));
        spans.push(Span::styled(" │", dim_style));
    }
    out.push(RenderLine {
        line: Line::from(spans),
        unit,
        raw_line: None, // atomic block
        atomic: true,
        fill: false,
    });
}

/// One table cell rendered with inline markdown (bold/code/links/…),
/// truncated and padded to exactly `width` display columns.
fn cell_spans(theme: &Theme, text: &str, base: Style, width: usize) -> Vec<Span<'static>> {
    let mut lines = collect_inlines(theme, text, base);
    let line = if lines.is_empty() {
        Line::default()
    } else {
        lines.remove(0)
    };
    let mut spans = Vec::new();
    let mut budget = width;
    for span in line.spans {
        let full = UnicodeWidthStr::width(span.content.as_ref());
        if budget >= full {
            budget -= full;
            spans.push(span);
        } else if budget > 0 {
            // Partial fit: cut the span at the display-column boundary.
            let mut shown = String::new();
            let mut used = 0;
            for ch in span.content.chars() {
                let cw = UnicodeWidthStr::width(ch.to_string().as_str());
                if used + cw > budget {
                    break;
                }
                used += cw;
                shown.push(ch);
            }
            budget -= used;
            if !shown.is_empty() {
                spans.push(Span::styled(shown, span.style));
            }
            break;
        } else {
            break;
        }
    }
    if budget > 0 {
        spans.push(Span::styled(" ".repeat(budget), Style::default()));
    }
    spans
}

fn table_border(left: &str, mid: &str, right: &str, widths: &[usize]) -> String {
    let mut s = String::from(left);
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            s.push_str(mid);
        }
        s.push_str(&"─".repeat(w + 2));
    }
    s.push_str(right);
    s
}

/// Cached inline rendering for one table cell, used both for column sizing
/// and for wrapping the cell body to the chosen column width.
struct TableCellLayout {
    lines: Vec<Line<'static>>,
    width: usize,
    min_width: usize,
}

impl TableCellLayout {
    fn new(theme: &Theme, text: &str, base: Style) -> Self {
        let lines = collect_inlines(theme, text, base);
        let rendered_width = lines.iter().map(|line| line.width()).max().unwrap_or(0);
        let width = rendered_width.max(1);
        let min_width = max_grapheme_width(text).max(1).min(width);
        Self {
            lines,
            width,
            min_width,
        }
    }
}

fn max_grapheme_width(text: &str) -> usize {
    text.grapheme_indices(true)
        .map(|(_, grapheme)| UnicodeWidthStr::width(grapheme))
        .max()
        .unwrap_or(0)
}

/// Shrink natural column widths until the whole boxed table fits `max_width`.
/// Columns never go below their widest grapheme so CJK/emoji do not split.
fn fit_table_widths(natural: &[usize], min: &[usize], max_width: usize) -> Vec<usize> {
    let cols = natural.len();
    if cols == 0 {
        return Vec::new();
    }
    let border = 3 * cols + 1;
    if max_width <= border {
        return min.to_vec();
    }
    let budget = max_width - border;
    let total_natural: usize = natural.iter().sum();
    if total_natural <= budget {
        return natural.to_vec();
    }
    let total_min: usize = min.iter().sum();
    if total_min >= budget {
        return min.to_vec();
    }
    let flexible_natural: usize = natural.iter().zip(min.iter()).map(|(n, m)| n - m).sum();
    let flexible_budget = budget - total_min;
    let mut widths: Vec<usize> = natural
        .iter()
        .zip(min.iter())
        .map(|(n, m)| {
            if flexible_natural == 0 {
                *m
            } else {
                m + (n - m) * flexible_budget / flexible_natural
            }
        })
        .collect();
    let mut overflow = widths.iter().sum::<usize>().saturating_sub(budget);
    let mut indices: Vec<usize> = (0..cols).collect();
    indices.sort_by_key(|&i| std::cmp::Reverse(widths[i] - min[i]));
    for i in indices {
        while overflow > 0 && widths[i] > min[i] {
            widths[i] -= 1;
            overflow -= 1;
        }
    }
    widths
}

/// Emit one table row as multiple visual lines when cells wrap. The row's
/// inline layouts are already styled; missing continuation rows are padded
/// with spaces so all vertical borders stay aligned.
fn push_table_cell_rows(
    out: &mut Vec<RenderLine>,
    unit: u64,
    dim_style: Style,
    widths: &[usize],
    row_layouts: &[TableCellLayout],
) {
    let mut cell_lines: Vec<Vec<Line<'static>>> = Vec::with_capacity(row_layouts.len());
    let mut height = 1usize;
    for (i, layout) in row_layouts.iter().enumerate() {
        let width = widths.get(i).copied().unwrap_or(1);
        let mut lines = Vec::new();
        for line in &layout.lines {
            if line.width() <= width {
                lines.push(line.clone());
            } else {
                lines.extend(wrap_styled_line(line.clone(), width));
            }
        }
        if lines.is_empty() {
            lines.push(Line::default());
        }
        height = height.max(lines.len());
        cell_lines.push(lines);
    }

    for row in 0..height {
        let mut spans = vec![Span::styled("\u{2502}", dim_style)];
        for (i, w) in widths.iter().enumerate() {
            spans.push(Span::styled(" ", dim_style));
            if let Some(lines) = cell_lines.get(i) {
                if let Some(line) = lines.get(row) {
                    spans.extend(line.spans.iter().cloned());
                    let used = line.width();
                    if used < *w {
                        spans.push(Span::styled(" ".repeat(*w - used), Style::default()));
                    }
                } else {
                    spans.push(Span::styled(" ".repeat(*w), Style::default()));
                }
            } else {
                spans.push(Span::styled(" ".repeat(*w), Style::default()));
            }
            spans.push(Span::styled(" \u{2502}", dim_style));
        }
        out.push(RenderLine {
            line: Line::from(spans),
            unit,
            raw_line: None, // atomic block
            atomic: true,
            fill: false,
        });
    }
}

/// Split one styled table-cell line with the shared greedy word wrapper,
/// preserving span styles and keeping grapheme clusters intact.
pub(super) fn wrap_styled_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    crate::wrap::wrap_line(line, width)
}
