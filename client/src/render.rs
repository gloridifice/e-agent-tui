//! Markdown renderer with source mapping (design §2.1, §3.3.3, D8–D11).
//!
//! Every block becomes a RenderUnit carrying its raw markdown source; every
//! rendered line remembers its unit and, where line-aligned, its raw line
//! number. Tables/mermaid/code blocks are atomic: any of their rendered rows
//! maps to the whole block (copy mode, M4).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::config::Theme;

/// Head/tail window sizes for collapsed atomic blocks.
const CODE_HEAD_ROWS: usize = 15;
const CODE_TAIL_ROWS: usize = 5;

/// One rendered screen line plus its provenance.
#[derive(Debug, Clone)]
pub struct RenderLine {
    pub line: Line<'static>,
    /// Owning render unit (stable id).
    pub unit: u64,
    /// Raw-source line this screen row corresponds to; `None` = atomic block
    /// (the whole raw block is selected).
    pub raw_line: Option<usize>,
    /// True for table/code/mermaid rows: the UI must not add the assistant
    /// bar prefix, and copy mode treats the block as one unit (D11).
    pub atomic: bool,
    /// Full-width background fill: the UI pads the row to the area width
    /// with Night (`bg`, glow-style code/mermaid blocks).
    pub fill: bool,
}

/// Block taxonomy for copy-mode semantics.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Paragraph,
    Heading,
    CodeBlock { lang: Option<String> },
    Table,
    List,
    Quote,
    Rule,
    Html,
}

impl BlockKind {
    /// Atomic blocks are copied whole (D11).
    pub fn is_atomic(&self) -> bool {
        matches!(self, BlockKind::Table | BlockKind::CodeBlock { .. })
    }
}

const MAX_CELL_WIDTH: usize = 40;

/// Per-render options (config-derived, design D28).
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Units whose collapsed window is expanded (D13).
    pub expanded: HashSet<u64>,
    /// Collapse threshold for atomic blocks in rows.
    pub collapse_rows: usize,
    /// Whether mermaid fences render via WASM (D9); off = raw fence.
    pub mermaid_enabled: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            expanded: HashSet::new(),
            collapse_rows: 40,
            mermaid_enabled: true,
        }
    }
}

/// Render markdown into styled lines with source mapping.
/// `next_unit` is an id allocator shared per app state; `units` receives the
/// raw markdown source of every rendered unit (keyed by id) for copy mode.
pub fn render_markdown(
    text: &str,
    theme: &Theme,
    next_unit: &mut u64,
    options: &RenderOptions,
    units: &mut HashMap<u64, String>,
) -> Vec<RenderLine> {
    let md_options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let parser = Parser::new_ext(text, md_options);
    let mut out = Vec::new();

    let mut block: Vec<(Event, Range<usize>)> = Vec::new();
    let mut block_kind: Option<BlockKind> = None;
    let mut depth: usize = 0;
    let mut block_start: usize = 0;

    for (event, range) in parser.into_offset_iter() {
        if block_kind.is_none() {
            // Start of a top-level block.
            let kind = top_level_kind(&event);
            if let Some(kind) = kind {
                block_kind = Some(kind);
                block_start = range.start;
                if matches!(event, Event::Rule | Event::Html(_)) {
                    // Self-contained single-event block.
                    let raw = &text[range.clone()];
                    let unit = *next_unit;
                    *next_unit += 1;
                    units.insert(unit, raw.to_string());
                    emit_block(
                        block_kind.as_ref().unwrap(),
                        raw,
                        unit,
                        theme,
                        options,
                        &mut out,
                    );
                    block_kind = None;
                    continue;
                }
                block.clear();
            }
        }
        if block_kind.is_some() {
            if matches!(event, Event::Start(ref tag) if is_block_tag(tag)) {
                depth += 1;
            }
            if matches!(event, Event::End(ref tag) if is_block_tag_end(tag)) {
                depth = depth.saturating_sub(1);
            }
            block.push((event, range.clone()));
            if depth == 0 && !block.is_empty() {
                let raw = &text[block_start..range.end];
                let unit = *next_unit;
                *next_unit += 1;
                units.insert(unit, raw.to_string());
                emit_block(
                    block_kind.as_ref().unwrap(),
                    raw,
                    unit,
                    theme,
                    options,
                    &mut out,
                );
                block.clear();
                block_kind = None;
            }
        }
    }
    // Unclosed trailing block (streaming safety).
    if block_kind.is_some() && !block.is_empty() {
        let unit = *next_unit;
        *next_unit += 1;
        units.insert(unit, text[block_start..].to_string());
        emit_block(
            block_kind.as_ref().unwrap(),
            &text[block_start..],
            unit,
            theme,
            options,
            &mut out,
        );
    }
    normalize_blanks(&mut out);
    out
}

/// Render one block and append it with glamour-style margins: a blank line
/// separates every top-level block; headings get their `block_suffix` blank
/// (glamour dark style).
fn emit_block(
    kind: &BlockKind,
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    let mut block_out = Vec::new();
    render_block(kind, raw, unit, theme, options, &mut block_out);
    if block_out.is_empty() {
        return;
    }
    let is_code = matches!(kind, BlockKind::CodeBlock { .. });
    if !out.is_empty() {
        push_blank(out, unit, is_code);
    }
    out.extend(block_out);
    if matches!(kind, BlockKind::Heading) {
        push_blank(out, unit, false);
    }
}

fn blank_row(unit: u64, atomic: bool) -> RenderLine {
    RenderLine {
        line: Line::default(),
        unit,
        raw_line: None,
        atomic,
        fill: false,
    }
}

/// Push one blank separator row (skipped when the previous row is blank).
fn push_blank(out: &mut Vec<RenderLine>, unit: u64, atomic: bool) {
    if out.last().map_or(false, |r| r.line.width() == 0) {
        return;
    }
    out.push(blank_row(unit, atomic));
}

/// Trim leading/trailing blank rows and collapse interior runs to at most
/// two (glamour's deepest margin), so transcript rows stay dense.
fn normalize_blanks(out: &mut Vec<RenderLine>) {
    let blank = |r: &RenderLine| r.line.width() == 0;
    while out.first().map_or(false, blank) {
        out.remove(0);
    }
    while out.last().map_or(false, blank) {
        out.pop();
    }
    let mut write = 0usize;
    let mut run = 0usize;
    for i in 0..out.len() {
        if blank(&out[i]) {
            run += 1;
        } else {
            run = 0;
        }
        if run <= 2 {
            if write != i {
                out.swap(write, i);
            }
            write += 1;
        }
    }
    out.truncate(write);
}

fn top_level_kind(event: &Event) -> Option<BlockKind> {
    match event {
        Event::Start(Tag::Paragraph) => Some(BlockKind::Paragraph),
        Event::Start(Tag::Heading { .. }) => Some(BlockKind::Heading),
        Event::Start(Tag::CodeBlock(kind)) => Some(BlockKind::CodeBlock {
            lang: match kind {
                pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                    if lang.is_empty() {
                        None
                    } else {
                        Some(lang.to_string())
                    }
                }
                pulldown_cmark::CodeBlockKind::Indented => None,
            },
        }),
        Event::Start(Tag::Table(_)) => Some(BlockKind::Table),
        Event::Start(Tag::List(_)) => Some(BlockKind::List),
        Event::Start(Tag::BlockQuote(_)) => Some(BlockKind::Quote),
        Event::Start(Tag::HtmlBlock) => Some(BlockKind::Html),
        Event::Rule => Some(BlockKind::Rule),
        Event::Html(_) => Some(BlockKind::Html),
        _ => None,
    }
}

fn is_block_tag(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::List(_)
            | Tag::Item
            | Tag::BlockQuote(_)
    )
}

/// Only block-level End events close a collected block; inline End events
/// (Emphasis, Strong, Link …) must not decrement the depth.
fn is_block_tag_end(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::BlockQuote(_)
    )
}

// ---------------------------------------------------------------------------
// Block renderers
// ---------------------------------------------------------------------------

fn render_block(
    kind: &BlockKind,
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    match kind {
        BlockKind::Paragraph => {
            let inlines = collect_inlines(theme, raw, Style::default().fg(theme.fg));
            for (i, line) in inlines.into_iter().enumerate() {
                out.push(RenderLine {
                    line,
                    unit,
                    raw_line: Some(i),
                    atomic: false,
                    fill: false,
                });
            }
        }
        BlockKind::Heading => {
            // glamour hides the `#` markers and styles by level (dark style).
            let level = heading_level(raw);
            let base = heading_style(theme, level);
            let stripped: String = raw
                .lines()
                .map(|l| {
                    let t = l.trim_start_matches('#').trim_start_matches(' ');
                    t.to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            let inlines = collect_inlines(theme, &stripped, base);
            for (i, line) in inlines.into_iter().enumerate() {
                let rendered = if level == 1 {
                    // h1: padded reverse bar (glamour h1 background look).
                    let mut spans = vec![Span::styled(" ", base)];
                    spans.extend(
                        line.spans
                            .into_iter()
                            .map(|s| Span::styled(s.content.into_owned(), base)),
                    );
                    spans.push(Span::styled(" ", base));
                    Line::from(spans)
                } else {
                    let mut spans = line.spans;
                    for s in spans.iter_mut() {
                        s.style = base.patch(s.style);
                    }
                    Line::from(spans)
                };
                out.push(RenderLine {
                    line: rendered,
                    unit,
                    raw_line: Some(i),
                    atomic: false,
                    fill: false,
                });
            }
        }
        BlockKind::Quote => {
            // Render per raw line: pulldown merges nested block quotes into
            // one paragraph, which would flatten the `>` levels. Each line
            // keeps its own depth of `│` bars (glamour indent_token).
            for (i, raw_line) in raw.lines().enumerate() {
                let (depth, content) = quote_depth(raw_line);
                let inlines = collect_inlines(theme, content, Style::default().fg(theme.fg));
                for line in inlines {
                    // One `│ ` pair per level (glamour indent_token).
                    let mut spans = vec![Span::styled(
                        "│ ".repeat(depth),
                        Style::default().fg(theme.dim),
                    )];
                    spans.extend(line.spans);
                    out.push(RenderLine {
                        line: Line::from(spans),
                        unit,
                        raw_line: Some(i),
                        atomic: false,
                        fill: false,
                    });
                }
            }
        }
        BlockKind::CodeBlock { lang } => {
            if lang.as_deref() == Some("mermaid") && options.mermaid_enabled {
                render_mermaid_block(raw, unit, theme, options, out);
            } else {
                render_code_block(raw, lang.as_deref(), unit, theme, options, out);
            }
        }
        BlockKind::Table => {
            render_table(raw, unit, theme, options, out);
        }
        BlockKind::List => {
            render_list(raw, unit, theme, out);
        }
        BlockKind::Rule => {
            out.push(RenderLine {
                line: Line::from(Span::styled("─".repeat(32), Style::default().fg(theme.dim))),
                unit,
                raw_line: Some(0),
                atomic: false,
                fill: false,
            });
        }
        BlockKind::Html => {
            for (i, line) in raw.lines().enumerate() {
                out.push(RenderLine {
                    line: Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(theme.dim),
                    )),
                    unit,
                    raw_line: Some(i),
                    atomic: false,
                    fill: false,
                });
            }
        }
    }
}

fn heading_level(raw: &str) -> usize {
    raw.chars().take_while(|c| *c == '#').count().max(1)
}

/// Nesting depth of a quote line ("a > b > c" = depth 3) and the content
/// after the markers.
fn quote_depth(line: &str) -> (usize, &str) {
    let mut rest = line;
    let mut depth = 0;
    while let Some(stripped) = rest.strip_prefix('>') {
        depth += 1;
        rest = stripped.strip_prefix(' ').unwrap_or(stripped);
    }
    (depth.max(1), rest.trim_start_matches(' '))
}

/// glamour dark-style heading variants mapped onto the ferra palette:
/// h1 = reverse bar (fg on user bg), h2 bold orange, h3 bold yellow,
/// h4 italic pink, h5 peach, h6 dim.
fn heading_style(theme: &Theme, level: usize) -> Style {
    match level {
        1 => Style::default()
            .fg(theme.bg)
            .bg(theme.user)
            .add_modifier(Modifier::BOLD),
        2 => Style::default().fg(theme.user).add_modifier(Modifier::BOLD),
        3 => Style::default()
            .fg(theme.running)
            .add_modifier(Modifier::BOLD),
        4 => Style::default()
            .fg(theme.rose)
            .add_modifier(Modifier::ITALIC),
        5 => Style::default().fg(theme.link),
        _ => Style::default().fg(theme.dim),
    }
}

/// Collect inline content of a paragraph/heading/quote block. Soft/hard
/// breaks split lines, matching the raw source's own line breaks. Inline
/// styling follows glamour dark: strong/emph turn pink, inline code is a
/// padded pink-on-soft chip, links render `text` (bold pink) followed by the
/// underlined URL, and a style stack keeps nesting correct.
fn collect_inlines(theme: &Theme, raw: &str, base: Style) -> Vec<Line<'static>> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut lines: Vec<Line<'static>> = vec![Line::default()];
    let mut style = base;
    let mut stack: Vec<Style> = Vec::new();
    // Pending link/image: (url, span index on the current line at start).
    let mut pending: Option<(String, usize)> = None;

    let parser = Parser::new_ext(raw, options);
    for event in parser {
        match event {
            Event::Text(t) => push_span(&mut lines, &style, &t),
            Event::Code(t) => {
                // glamour code: prefix/suffix space, pink on Night bg.
                let code_style = Style::default().fg(theme.rose).bg(theme.bg);
                let last = lines.last_mut().unwrap();
                last.push_span(Span::styled(" ", code_style));
                last.push_span(Span::styled(t.to_string(), code_style));
                last.push_span(Span::styled(" ", code_style));
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                push_span(&mut lines, &style, &h);
            }
            Event::SoftBreak | Event::HardBreak => lines.push(Line::default()),
            Event::Start(tag) => match tag {
                Tag::Emphasis => {
                    stack.push(style);
                    style = style.fg(theme.rose).add_modifier(Modifier::ITALIC);
                }
                Tag::Strong => {
                    stack.push(style);
                    style = style.fg(theme.rose).add_modifier(Modifier::BOLD);
                }
                Tag::Strikethrough => {
                    stack.push(style);
                    style = style.add_modifier(Modifier::CROSSED_OUT);
                }
                Tag::Link { dest_url, .. } => {
                    stack.push(style);
                    pending = Some((dest_url.to_string(), lines.last().unwrap().spans.len()));
                    style = style.fg(theme.rose).add_modifier(Modifier::BOLD);
                }
                Tag::Image { dest_url, .. } => {
                    stack.push(style);
                    pending = Some((dest_url.to_string(), lines.last().unwrap().spans.len()));
                    style = style.fg(theme.link).add_modifier(Modifier::ITALIC);
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    style = stack.pop().unwrap_or(base);
                }
                TagEnd::Link | TagEnd::Image => {
                    // glamour renders the URL after the link text, underlined.
                    if let Some((url, start_idx)) = pending.take() {
                        if !url.is_empty() {
                            let spans = &lines.last().unwrap().spans;
                            let text: String = spans[start_idx.min(spans.len())..]
                                .iter()
                                .map(|s| s.content.as_ref())
                                .collect();
                            if text != url {
                                lines.last_mut().unwrap().push_span(Span::styled(" ", base));
                                lines.last_mut().unwrap().push_span(Span::styled(
                                    url,
                                    Style::default()
                                        .fg(theme.link)
                                        .add_modifier(Modifier::UNDERLINED),
                                ));
                            }
                        }
                    }
                    style = stack.pop().unwrap_or(base);
                }
                _ => {}
            },
            _ => {}
        }
    }
    // Drop trailing empty line artifacts.
    while lines.last().map_or(false, |l| l.width() == 0) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn push_span(lines: &mut [Line<'static>], style: &Style, text: &str) {
    let span = Span::styled(text.to_string(), *style);
    lines.last_mut().unwrap().push_span(span);
}

// ---------------------------------------------------------------------------
// Mermaid (D9): grok-mermaid WASM -> styled box art; trap falls back to the
// fenced source. The block stays atomic: copy mode yields the raw mermaid.
// ---------------------------------------------------------------------------

fn render_mermaid_block(
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let source = if raw_lines.len() >= 2 {
        raw_lines[1..raw_lines.len() - 1].join("\n")
    } else {
        raw_lines.join("\n")
    };
    let dim = Style::default().fg(theme.dim);
    // Glow-style header: `  mermaid · N 行` on the filled block.
    out.push(RenderLine {
        line: Line::from(vec![
            Span::styled("  ", dim),
            Span::styled("mermaid", dim),
            Span::styled(format!(" · {} 行", source.lines().count()), dim),
        ]),
        unit,
        raw_line: Some(0),
        atomic: true,
        fill: true,
    });

    match crate::mermaid::render(&source, 0) {
        Ok(diagram) => {
            let collapsed =
                !options.expanded.contains(&unit) && diagram.len() > options.collapse_rows;
            let emit = |i: usize, out: &mut Vec<RenderLine>| {
                let line = &diagram[i];
                let spans: Vec<Span<'static>> = line
                    .spans
                    .iter()
                    .map(|s| {
                        let style = match s.class {
                            crate::mermaid::MermaidClass::Border => Style::default().fg(theme.dim),
                            crate::mermaid::MermaidClass::Node => Style::default().fg(theme.fg),
                            crate::mermaid::MermaidClass::Edge => Style::default().fg(theme.dim),
                            crate::mermaid::MermaidClass::EdgeLabel => {
                                Style::default().fg(theme.link)
                            }
                            crate::mermaid::MermaidClass::Title => {
                                Style::default().fg(theme.user).add_modifier(Modifier::BOLD)
                            }
                        };
                        Span::styled(s.text.clone(), style)
                    })
                    .collect();
                let mut base = vec![Span::styled("  ", dim)];
                base.extend(spans);
                out.push(RenderLine {
                    line: Line::from(base),
                    unit,
                    raw_line: None,
                    atomic: true,
                    fill: true,
                });
            };
            if collapsed {
                let head = CODE_HEAD_ROWS.min(diagram.len());
                for i in 0..head {
                    emit(i, out);
                }
                let hidden = diagram.len() - head - CODE_TAIL_ROWS;
                out.push(collapse_hint_row(unit, theme, hidden));
                for i in (diagram.len() - CODE_TAIL_ROWS.min(diagram.len()))..diagram.len() {
                    emit(i, out);
                }
            } else {
                for i in 0..diagram.len() {
                    emit(i, out);
                }
            }
        }
        Err(error) => {
            // Fall back to the fenced source (D9).
            out.push(RenderLine {
                line: Line::from(vec![
                    Span::styled("  ", dim),
                    Span::styled(
                        format!("(mermaid 渲染失败: {error})"),
                        Style::default().fg(theme.dim),
                    ),
                ]),
                unit,
                raw_line: None,
                atomic: true,
                fill: true,
            });
            for (i, line) in raw_lines.iter().skip(1).enumerate() {
                out.push(RenderLine {
                    line: Line::from(vec![
                        Span::styled("  ", dim),
                        Span::styled((*line).to_string(), Style::default().fg(theme.fg)),
                    ]),
                    unit,
                    raw_line: Some(i + 1),
                    atomic: true,
                    fill: true,
                });
            }
        }
    }
    block_bottom_pad(unit, theme, out);
}

// ---------------------------------------------------------------------------
// Code block
// ---------------------------------------------------------------------------

fn render_code_block(
    raw: &str,
    lang: Option<&str>,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    // raw includes the fence; content lines are raw[1..last].
    let raw_lines: Vec<&str> = raw.lines().collect();
    let (content, fence_offset) = if raw_lines.len() >= 2 {
        (&raw_lines[1..raw_lines.len() - 1], 1)
    } else {
        (&raw_lines[..], 0)
    };
    let dim = Style::default().fg(theme.dim);
    // Glow-style header: `  lang · N 行` — no frame.
    out.push(RenderLine {
        line: Line::from(vec![
            Span::styled("  ", dim),
            Span::styled(lang.unwrap_or("code").to_string(), dim),
            Span::styled(format!(" · {} 行", content.len()), dim),
        ]),
        unit,
        raw_line: Some(0),
        atomic: true,
        fill: true,
    });

    let collapsed = !options.expanded.contains(&unit) && content.len() > options.collapse_rows;
    let push_content = |i: usize, out: &mut Vec<RenderLine>| {
        out.push(RenderLine {
            line: Line::from(vec![
                Span::styled("  ", dim),
                Span::styled(content[i].to_string(), Style::default().fg(theme.fg)),
            ]),
            unit,
            raw_line: Some(i + fence_offset),
            atomic: true,
            fill: true,
        });
    };
    if collapsed {
        for i in 0..CODE_HEAD_ROWS.min(content.len()) {
            push_content(i, out);
        }
        let hidden = content.len() - CODE_HEAD_ROWS - CODE_TAIL_ROWS;
        out.push(collapse_hint_row(unit, theme, hidden));
        for i in (content.len() - CODE_TAIL_ROWS)..content.len() {
            push_content(i, out);
        }
    } else {
        for i in 0..content.len() {
            push_content(i, out);
        }
    }
    block_bottom_pad(unit, theme, out);
}

/// The collapsed-window hint row (shared by code and mermaid blocks).
fn collapse_hint_row(unit: u64, theme: &Theme, hidden: usize) -> RenderLine {
    RenderLine {
        line: Line::from(Span::styled(
            format!("  … 收起 {hidden} 行 [Enter 展开]"),
            Style::default().fg(theme.dim),
        )),
        unit,
        raw_line: None,
        atomic: true,
        fill: true,
    }
}

/// One row of inner padding below a code/mermaid block: flagged `fill` so the
/// UI paints it with the block background, and carrying a single backgrounded
/// space so blank-normalization keeps it (a width-0 row would be trimmed as
/// a glamour margin blank at the message end).
fn block_bottom_pad(unit: u64, theme: &Theme, out: &mut Vec<RenderLine>) {
    out.push(RenderLine {
        line: Line::from(Span::styled(" ", Style::default().bg(theme.bg))),
        unit,
        raw_line: None,
        atomic: true,
        fill: true,
    });
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

fn render_table(
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
                line: Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(theme.fg),
                )),
                unit,
                raw_line: Some(i),
                atomic: false,
                fill: false,
            });
        }
        return;
    }

    let cols = rows.iter().map(Vec::len).max().unwrap_or(1);
    let mut widths: Vec<usize> = vec![0; cols];
    for row in &rows {
        for (c, cell) in row.iter().enumerate() {
            let w = UnicodeWidthStr::width(cell.as_str());
            widths[c] = widths[c].max(w.min(MAX_CELL_WIDTH));
        }
    }
    let has_header = rows.len() >= 2
        && rows[1]
            .iter()
            .all(|c| c.chars().all(|ch| ch == '-' || ch == ':'));

    let dim_style = Style::default().fg(theme.dim);
    let body_rows: Vec<usize> = (0..rows.len())
        .filter(|r| !(*r == 1 && has_header))
        .collect();
    let collapsed =
        !options.expanded.contains(&unit) && body_rows.len() > options.collapse_rows / 2;
    push_plain(out, unit, table_border("┌", "┬", "┐", &widths), dim_style);
    if collapsed {
        // Header + separator + first rows … last rows.
        push_table_cells(out, unit, dim_style, &widths, &rows[0], true, theme);
        push_plain(out, unit, table_border("├", "┼", "┤", &widths), dim_style);
        let shown = 3usize.min(body_rows.len().saturating_sub(1));
        for idx in &body_rows[1..1 + shown] {
            push_table_cells(out, unit, dim_style, &widths, &rows[*idx], false, theme);
        }
        let hidden = body_rows.len() - shown - 1 - 2;
        let hint = format!("│ … 收起 {hidden} 行 [Enter 展开]");
        push_plain(out, unit, hint, dim_style);
        for idx in &body_rows[body_rows.len() - 2..] {
            push_table_cells(out, unit, dim_style, &widths, &rows[*idx], false, theme);
        }
    } else {
        for (r, row) in rows.iter().enumerate() {
            if r == 1 && has_header {
                push_plain(out, unit, table_border("├", "┼", "┤", &widths), dim_style);
                continue;
            }
            push_table_cells(out, unit, dim_style, &widths, row, r == 0, theme);
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
        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
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

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// One list item plus its nested sub-items (children complete first in
/// pulldown's event order, so they attach to their parent instead of
/// emitting out of order).
struct ItemBuf {
    text: String,
    task: Option<bool>,
    children: Vec<ItemBuf>,
}

fn render_list(raw: &str, unit: u64, theme: &Theme, out: &mut Vec<RenderLine>) {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(raw, options);
    let mut ordered: Option<u64> = None;
    // Number counters per nesting depth.
    let mut counters: Vec<u64> = Vec::new();
    let mut item_stack: Vec<ItemBuf> = Vec::new();
    let mut raw_line_no = 0;

    for event in parser {
        match event {
            Event::Start(Tag::List(start)) => {
                ordered = start;
                counters.clear();
            }
            Event::Start(Tag::Item) => item_stack.push(ItemBuf {
                text: String::new(),
                task: None,
                children: Vec::new(),
            }),
            Event::TaskListMarker(checked) => {
                if let Some(item) = item_stack.last_mut() {
                    item.task = Some(checked);
                }
            }
            Event::End(TagEnd::Item) => {
                let Some(item) = item_stack.pop() else {
                    continue;
                };
                if let Some(parent) = item_stack.last_mut() {
                    // Nested item: attach to its parent, emit with the tree.
                    parent.children.push(item);
                } else {
                    emit_item_tree(
                        item,
                        0,
                        ordered,
                        &mut counters,
                        &mut raw_line_no,
                        theme,
                        unit,
                        out,
                    );
                }
            }
            Event::Text(t) => {
                if let Some(item) = item_stack.last_mut() {
                    item.text.push_str(&t);
                }
            }
            Event::Code(t) => {
                if let Some(item) = item_stack.last_mut() {
                    item.text.push_str(&t);
                }
            }
            Event::SoftBreak => {
                if let Some(item) = item_stack.last_mut() {
                    item.text.push(' ');
                }
            }
            Event::End(TagEnd::List(_)) => ordered = None,
            _ => {}
        }
    }
    // Unclosed items (streaming safety).
    while let Some(item) = item_stack.pop() {
        let depth = item_stack.len();
        emit_item_tree(
            item,
            depth,
            ordered,
            &mut counters,
            &mut raw_line_no,
            theme,
            unit,
            out,
        );
    }
}

/// Emit an item (marker + inline-styled text), then its children one level
/// deeper — source order, glamour 2-column indent per level.
fn emit_item_tree(
    item: ItemBuf,
    depth: usize,
    ordered: Option<u64>,
    counters: &mut Vec<u64>,
    raw_line_no: &mut usize,
    theme: &Theme,
    unit: u64,
    out: &mut Vec<RenderLine>,
) {
    emit_list_item(
        item.text,
        item.task,
        depth,
        ordered,
        counters,
        raw_line_no,
        theme,
        unit,
        out,
    );
    for child in item.children {
        emit_item_tree(
            child,
            depth + 1,
            ordered,
            counters,
            raw_line_no,
            theme,
            unit,
            out,
        );
    }
}

/// Render one list item: task checkbox or `◦`/numbered marker, inline
/// markdown inside.
fn emit_list_item(
    text: String,
    task: Option<bool>,
    depth: usize,
    ordered: Option<u64>,
    counters: &mut Vec<u64>,
    raw_line_no: &mut usize,
    theme: &Theme,
    unit: u64,
    out: &mut Vec<RenderLine>,
) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let indent = "  ".repeat(depth);
    let (marker, marker_style) = match task {
        // glamour task: "[✓]" / "[ ]" followed by the item text.
        Some(true) => ("[✓] ".to_string(), Style::default().fg(theme.ok)),
        Some(false) => ("[ ] ".to_string(), Style::default().fg(theme.dim)),
        None => {
            if ordered.is_some() {
                while counters.len() <= depth {
                    counters.push(0);
                }
                counters[depth] += 1;
                (
                    format!("{}. ", counters[depth]),
                    Style::default().fg(theme.user),
                )
            } else {
                (
                    "◦ ".to_string(),
                    Style::default().fg(if depth == 0 { theme.user } else { theme.dim }),
                )
            }
        }
    };
    counters.truncate(depth + 1);
    // Inline markdown inside items (code, strong, links …).
    let inlines = collect_inlines(theme, &text, Style::default().fg(theme.fg));
    for (li, line) in inlines.into_iter().enumerate() {
        let mut spans = Vec::new();
        if li == 0 {
            spans.push(Span::styled(indent.clone(), Style::default().fg(theme.dim)));
            spans.push(Span::styled(marker.clone(), marker_style));
            spans.extend(line.spans);
        } else {
            // Continuation rows align under the text column.
            spans.push(Span::styled(
                format!("{indent}  "),
                Style::default().fg(theme.dim),
            ));
            spans.extend(line.spans);
        }
        out.push(RenderLine {
            line: Line::from(spans),
            unit,
            raw_line: Some(*raw_line_no),
            atomic: false,
            fill: false,
        });
        *raw_line_no += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str) -> Vec<RenderLine> {
        render_full(text).0
    }

    fn render_full(text: &str) -> (Vec<RenderLine>, HashMap<u64, String>) {
        let theme = Theme::ferra();
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            collapse_rows: 40,
            ..Default::default()
        };
        let lines = render_markdown(text, &theme, &mut next, &options, &mut units);
        (lines, units)
    }

    fn plain(lines: &[RenderLine]) -> Vec<String> {
        lines
            .iter()
            .map(|r| {
                r.line
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn table_renders_boxed_and_atomic() {
        let lines = render("| a | b |\n|---|---|\n| 1 | 2 |");
        let text = plain(&lines);
        assert!(
            text[0].starts_with("┌") && text[0].contains("┬"),
            "top border: {}",
            text[0]
        );
        assert!(text.iter().any(|l| l.starts_with("├")), "header separator");
        assert!(text.last().unwrap().starts_with("└"), "bottom border");
        assert!(lines.iter().all(|r| r.atomic), "all table rows atomic");
        assert!(
            lines.iter().all(|r| r.raw_line.is_none()),
            "no row-level mapping for table"
        );
    }

    /// Regression: bold and inline code inside table cells used to leak their
    /// markdown markers (`**`/backticks) instead of rendering styled.
    #[test]
    fn table_cell_renders_bold_and_code() {
        let lines = render("| a | b |\n|---|---|\n| **粗体** | `code` |");
        let text = plain(&lines);
        assert!(
            !text.iter().any(|l| l.contains("**")),
            "bold markers hidden: {text:?}"
        );
        assert!(
            !text.iter().any(|l| l.contains('`')),
            "backticks hidden: {text:?}"
        );
        let bold = lines
            .iter()
            .flat_map(|r| r.line.spans.iter())
            .find(|s| s.content == "粗体")
            .expect("bold span rendered");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        let code = lines
            .iter()
            .flat_map(|r| r.line.spans.iter())
            .find(|s| s.content == "code")
            .expect("code span rendered");
        assert_eq!(code.style.bg, Some(Theme::ferra().bg));
        assert_eq!(code.style.fg, Some(Theme::ferra().rose));
    }

    #[test]
    fn table_cell_truncates_to_column_width() {
        let long = "x".repeat(50);
        let lines = render(&format!("| c |\n|---|\n| {long} |"));
        let row = lines
            .iter()
            .find(|r| r.line.spans.iter().any(|s| s.content.starts_with('x')))
            .expect("content row present");
        // │ + ' ' + cell(40) + ' │' → the cell itself must be exactly 40 wide.
        let total: usize = row
            .line
            .spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();
        assert_eq!(
            total,
            1 + 1 + MAX_CELL_WIDTH + 2,
            "cell padded to 40 columns"
        );
    }

    #[test]
    fn code_block_fence_mapping() {
        let raw = "```rust\nfn main() {}\n```";
        let lines = render(raw);
        assert!(lines.iter().all(|r| r.atomic));
        // Glow-style header: `  rust · 1 行` (no frame).
        assert_eq!(lines[0].line.spans[1].content, "rust", "lang label");
        assert!(lines[0].fill, "header fills its background");
        assert_eq!(lines[1].line.spans[1].content, "fn main() {}");
        assert!(lines[1].fill, "content fills its background");
        // The first content row maps to raw line 1 (line 0 is the fence).
        assert_eq!(lines[1].raw_line, Some(1));
        // One inner-padding row closes the block (backgrounded space, the UI
        // paints it full-width with the block background).
        assert_eq!(lines.len(), 3, "header + content + bottom padding");
        assert!(lines[2].fill, "padding row fills its background");
        assert_eq!(lines[2].line.width(), 1, "padding row is one space");
        assert_eq!(lines[2].raw_line, None, "padding maps to no source line");
    }

    #[test]
    fn heading_strips_markers_and_uses_level_colors() {
        let lines = render("## 标题");
        let text = plain(&lines);
        assert_eq!(text[0], "标题", "glamour hides the # markers");
        let span = &lines[0].line.spans[0];
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
        // h1 = reverse bar: fg = bg, bg = user.
        let h1 = render("# 一级");
        let span = &h1[0].line.spans[0];
        assert_eq!(span.content, " ", "h1 padded with a leading space");
        assert_eq!(span.style.bg, Some(Theme::ferra().user));
        assert_eq!(span.style.fg, Some(Theme::ferra().bg));
        // h3 = yellow, not bold-pink.
        let h3 = render("### 三级");
        assert_eq!(h3[0].line.spans[0].style.fg, Some(Theme::ferra().running));
    }

    #[test]
    fn link_renders_text_and_underlined_url() {
        let lines = render("看 [文档](https://x.dev) 吧");
        let spans = &lines[0].line.spans;
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("https://x.dev"),
            "url shown after the text: {text}"
        );
        let url = spans
            .iter()
            .find(|s| s.content == "https://x.dev")
            .expect("url span");
        assert!(url.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn autolink_does_not_duplicate_url() {
        let lines = render("see <https://x.dev> now");
        let text: String = lines[0]
            .line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(text.matches("https://x.dev").count(), 1, "got: {text}");
    }

    #[test]
    fn inline_code_is_padded_chip() {
        let lines = render("run `cargo` now");
        let code = lines[0]
            .line
            .spans
            .iter()
            .find(|s| s.content == "cargo")
            .expect("code span");
        assert_eq!(code.style.bg, Some(Theme::ferra().bg));
        let text: String = lines[0]
            .line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains(" cargo "), "padded spaces: {text}");
    }

    #[test]
    fn task_list_checkboxes() {
        let lines = render("- [x] 完成\n- [ ] 待办");
        let text = plain(&lines);
        assert_eq!(text[0], "[✓] 完成");
        assert_eq!(text[1], "[ ] 待办");
        assert_eq!(
            lines[0].line.spans[1].style.fg,
            Some(Theme::ferra().ok),
            "ticked box green"
        );
    }

    #[test]
    fn nested_list_indents_two_per_level() {
        let lines = render("- 一级\n  - 二级\n    - 三级");
        let text = plain(&lines);
        assert_eq!(text[0], "◦ 一级");
        assert_eq!(text[1], "  ◦ 二级");
        assert_eq!(text[2], "    ◦ 三级");
    }

    #[test]
    fn ordered_list_numbers_per_level() {
        let lines = render("1. 甲\n2. 乙");
        let text = plain(&lines);
        assert_eq!(text[0], "1. 甲");
        assert_eq!(text[1], "2. 乙");
    }

    #[test]
    fn nested_quote_stacks_bars() {
        let lines = render("> 外层\n> > 内层");
        let text = plain(&lines);
        assert_eq!(text[0], "│ 外层");
        assert_eq!(text[1], "│ │ 内层");
    }

    #[test]
    fn code_block_gets_blank_separators() {
        let lines = render("前文\n\n```\ncode\n```\n\n后文");
        let text = plain(&lines);
        // Glow-style block: header row + content row, no frame.
        let header_idx = text
            .iter()
            .position(|l| l == "  code · 1 行")
            .expect("glow header");
        assert_eq!(text[header_idx - 1], "", "blank row above code block");
        assert_eq!(text[header_idx + 1], "  code", "content row follows");
        assert_eq!(
            text[header_idx + 2],
            " ",
            "inner padding row below the block"
        );
        assert_eq!(text[header_idx + 3], "", "blank row below code block");
        assert_eq!(text[header_idx + 4], "后文", "next paragraph follows");
    }

    #[test]
    fn emphasis_is_styled() {
        let lines = render("a **bold** b");
        let bold = lines[0]
            .line
            .spans
            .iter()
            .find(|s| s.content == "bold")
            .expect("bold span");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn list_items_get_markers() {
        let lines = render("- one\n- two");
        let text = plain(&lines);
        assert_eq!(text[0], "◦ one");
        assert_eq!(text[1], "◦ two");
        assert_eq!(lines[0].raw_line, Some(0));
        assert_eq!(lines[1].raw_line, Some(1));
    }

    #[test]
    fn quote_prefix_and_line_map() {
        let lines = render("> first\n> second");
        let text = plain(&lines);
        assert!(text[0].starts_with("│ first"), "got: {}", text[0]);
        assert_eq!(lines[0].raw_line, Some(0));
        assert_eq!(lines[1].raw_line, Some(1));
    }

    #[test]
    fn paragraph_soft_break_maps_lines() {
        let lines = render("line one\nline two");
        assert_eq!(plain(&lines)[0], "line one");
        assert_eq!(plain(&lines)[1], "line two");
        assert_eq!(lines[0].raw_line, Some(0));
        assert_eq!(lines[1].raw_line, Some(1));
    }

    #[test]
    fn table_raw_preserved_for_copy() {
        // The raw source travels with the unit; here we just verify the
        // parser handed the table renderer the full raw text (visible through
        // the exact cell contents).
        let lines = render("| 名称 | 值 |\n|---|---|\n| x | 1 |");
        let text = plain(&lines);
        assert!(text.iter().any(|l| l.contains("名称")), "cjk cell kept");
    }

    #[test]
    fn units_carry_raw_source() {
        let (lines, units) = render_full("| a |\n|---|\n| 1 |");
        let unit = lines[0].unit;
        let raw = units.get(&unit).expect("raw kept");
        assert_eq!(raw, "| a |\n|---|\n| 1 |");
    }

    #[test]
    fn code_block_collapses_when_long() {
        let mut code = String::from("```rust\n");
        for i in 0..100 {
            code.push_str(&format!("line {i}\n"));
        }
        code.push_str("```\n");
        let (lines, _) = render_full(&code);
        let text = plain(&lines);
        assert!(
            text.iter().any(|l| l.contains("[Enter 展开]")),
            "collapse hint present: {:?}",
            text
        );
        assert!(lines.len() < 40, "collapsed: {} lines", lines.len());
    }

    #[test]
    fn expanded_code_block_shows_all() {
        let mut code = String::from("```\n");
        for i in 0..100 {
            code.push_str(&format!("line {i}\n"));
        }
        code.push_str("```\n");
        let theme = Theme::ferra();
        let mut next = 0;
        let mut units = HashMap::new();
        // Find the code unit id by rendering collapsed first.
        let collapsed = render_markdown(
            &code,
            &theme,
            &mut next,
            &RenderOptions::default(),
            &mut units,
        );
        let unit = collapsed[0].unit;
        let mut options = RenderOptions::default();
        options.expanded.insert(unit);
        // Re-render with the SAME unit range (unit 0), as the app does.
        let lines = render_markdown(&code, &theme, &mut 0, &options, &mut units);
        let text = plain(&lines);
        assert!(
            !text.iter().any(|l| l.contains("[Enter 展开]")),
            "no hint when expanded"
        );
        // Glow layout: header row + 100 content rows + 1 bottom padding row.
        assert_eq!(lines.len(), 102);
    }
}
