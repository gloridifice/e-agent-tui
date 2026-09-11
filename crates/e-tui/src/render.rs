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
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    config::{Config, Theme},
    i18n::Language,
};

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
    CodeBlock { lang: Option<String>, fenced: bool },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownStrength {
    #[default]
    Normal,
    Weak,
}

/// Per-render options (config-derived, design D28).
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Language used for frontend-owned Markdown chrome.
    pub language: Language,
    /// Units whose collapsed window is expanded (D13).
    pub expanded: HashSet<u64>,
    /// Collapse threshold for atomic blocks in rows.
    pub collapse_rows: usize,
    /// Whether mermaid fences render via WASM (D9); off = raw fence.
    pub mermaid_enabled: bool,
    /// Semantic Markdown palette used by this materialization.
    pub markdown_strength: MarkdownStrength,
    /// Optional display width of the surface these lines are painted into.
    /// When set, tables size their columns to fit it and wrap cell text
    /// inside the box, and list items pre-wrap with a hanging indent so
    /// continuation rows stay in the text column. `None` renders unbounded
    /// logical rows and leaves wrapping to the paint-time wrapper.
    pub content_width: Option<usize>,
    pub link_tags: Vec<crate::link_copy::TaggedLink>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            language: Language::English,
            expanded: HashSet::new(),
            collapse_rows: 40,
            mermaid_enabled: true,
            markdown_strength: MarkdownStrength::Normal,
            content_width: None,
            link_tags: Vec::new(),
        }
    }
}

pub(crate) fn transcript_options(
    config: &Config,
    expanded: &HashSet<u64>,
    content_width: usize,
) -> RenderOptions {
    RenderOptions {
        language: config.language,
        expanded: expanded.clone(),
        collapse_rows: config.atomic_collapse_rows,
        mermaid_enabled: config.mermaid_enabled,
        markdown_strength: MarkdownStrength::Normal,
        content_width: Some(content_width),
        link_tags: Vec::new(),
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
    // Keep the renderer's existing single `theme.markdown` access path while
    // selecting the surface-specific semantic group once per materialization.
    let mut selected_theme = *theme;
    if options.markdown_strength == MarkdownStrength::Weak {
        selected_theme.markdown = selected_theme.markdown_weak;
        selected_theme.code = selected_theme.code_weak;
        // Top-level bullets historically use the flat Coral alias. Weak
        // Markdown has no extra role, so route that derived accent through its
        // list-marker semantic instead of leaking a strong palette color.
        selected_theme.coral = selected_theme.markdown_weak.list_marker.fg;
    }
    let theme = &selected_theme;
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
            fenced: matches!(kind, pulldown_cmark::CodeBlockKind::Fenced(_)),
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
            let inlines =
                collect_inlines(theme, raw, theme.markdown.text.style(), &options.link_tags);
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
            let inlines = collect_inlines(theme, &stripped, base, &[]);
            for (i, line) in inlines.into_iter().enumerate() {
                let mut rendered = if level == 1 {
                    // h1 keeps horizontal padding; the semantic style decides
                    // whether that becomes a background bar.
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
                crate::link_copy::annotate(
                    &mut rendered,
                    &options.link_tags,
                    Style::default().fg(theme.activity.label.fg),
                );
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
            render_quote(raw, unit, theme, options, out);
        }
        BlockKind::CodeBlock { lang, fenced } => {
            if lang.as_deref() == Some("mermaid") && options.mermaid_enabled {
                render_mermaid_block(raw, unit, theme, options, out);
            } else {
                render_code_block(raw, lang.as_deref(), *fenced, unit, theme, options, out);
            }
            for line in out.iter_mut() {
                crate::link_copy::annotate(
                    &mut line.line,
                    &options.link_tags,
                    Style::default().fg(theme.activity.label.fg),
                );
            }
        }
        BlockKind::Table => {
            render_table(raw, unit, theme, options, out);
        }
        BlockKind::List => {
            render_list(raw, unit, theme, options, out);
        }
        BlockKind::Rule => {
            out.push(RenderLine {
                line: Line::from(Span::styled("─".repeat(32), theme.markdown.rule.style())),
                unit,
                raw_line: Some(0),
                atomic: false,
                fill: false,
            });
        }
        BlockKind::Html => {
            for (i, line) in raw.lines().enumerate() {
                out.push(RenderLine {
                    line: Line::from(Span::styled(line.to_string(), theme.code.meta.style())),
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

/// Render a quote block per raw line: pulldown merges nested block quotes into
/// one paragraph, which would flatten the `>` levels, so each line keeps its
/// own depth of `│` bars (glamour indent_token). Over-wide lines wrap against
/// the resolved content width and re-emit the same bars, so the quote gutter
/// stays contiguous instead of breaking where a row wrapped.
fn render_quote(
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    for (index, raw_line) in raw.lines().enumerate() {
        let (depth, content) = quote_depth(raw_line);
        // One `│ ` pair per level (glamour indent_token).
        let bars = "│ ".repeat(depth);
        let gutter = UnicodeWidthStr::width(bars.as_str());
        let body_width = options
            .content_width
            .map(|width| width.saturating_sub(gutter))
            .filter(|width| *width > 0);
        let inlines = collect_inlines(
            theme,
            content,
            theme.markdown.text.style(),
            &options.link_tags,
        );
        for line in inlines {
            let rows = match body_width {
                Some(width) => wrap_styled_line(line, width),
                None => vec![line],
            };
            for row in rows {
                let mut spans = vec![Span::styled(
                    bars.clone(),
                    theme.markdown.quote_marker.style(),
                )];
                spans.extend(row.spans);
                out.push(RenderLine {
                    line: Line::from(spans),
                    unit,
                    // Wrapped rows stay on their quote line's source line.
                    raw_line: Some(index),
                    atomic: false,
                    fill: false,
                });
            }
        }
    }
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

/// Heading styles come directly from the fixed Markdown semantic roles.
fn heading_style(theme: &Theme, level: usize) -> Style {
    match level {
        1 => theme.markdown.heading1.style(),
        2 => theme.markdown.heading2.style(),
        3 => theme.markdown.heading3.style(),
        4 => theme.markdown.heading4.style(),
        5 => theme.markdown.heading5.style(),
        _ => theme.markdown.heading6.style(),
    }
}

#[path = "render/inline.rs"]
mod inline;
use inline::{collect_inlines, InlineBuilder, SoftBreak};

#[path = "render/code.rs"]
mod code;
use code::{render_code_block, render_mermaid_block};

#[path = "render/table.rs"]
mod table;
use table::{render_table, wrap_styled_line};

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// One open list level. An ordered level hands out the source's own numbers,
/// so `3.` really starts at 3 and a nested level keeps its own sequence.
struct ListLevel {
    next: Option<u64>,
}

impl ListLevel {
    fn take_number(&mut self) -> Option<u64> {
        let current = self.next?;
        self.next = Some(current + 1);
        Some(current)
    }
}

/// One list item plus its nested sub-items (children complete first in
/// pulldown's event order, so they attach to their parent instead of
/// emitting out of order). Inline content is built from the list's own parser
/// events, never re-parsed from flattened text.
struct ItemBuf<'a> {
    inline: InlineBuilder<'a>,
    task: Option<bool>,
    /// Ordered-list number handed out by the owning level; `None` = bullet.
    number: Option<u64>,
    children: Vec<ItemBuf<'a>>,
}

/// Per-list emit state: the running raw-line cursor and the resolved content
/// width that gives every item its hanging indent.
struct ListRenderer<'a> {
    theme: &'a Theme,
    unit: u64,
    /// Resolved content width; `None` emits unwrapped logical rows.
    width: Option<usize>,
    raw_line_no: usize,
}

fn render_list(
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    let md_options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(raw, md_options);
    let mut levels: Vec<ListLevel> = Vec::new();
    let mut item_stack: Vec<ItemBuf> = Vec::new();
    let mut list = ListRenderer {
        theme,
        unit,
        width: options.content_width,
        raw_line_no: 0,
    };

    for event in parser {
        match &event {
            Event::Start(Tag::List(start)) => levels.push(ListLevel { next: *start }),
            Event::End(TagEnd::List(_)) => {
                levels.pop();
            }
            Event::Start(Tag::Item) => {
                let number = levels.last_mut().and_then(ListLevel::take_number);
                item_stack.push(ItemBuf {
                    // A list item wraps for itself, so its source line breaks
                    // are word separators rather than display rows.
                    inline: InlineBuilder::new(
                        theme,
                        theme.markdown.text.style(),
                        SoftBreak::Space,
                        &options.link_tags,
                    ),
                    task: None,
                    number,
                    children: Vec::new(),
                });
            }
            Event::TaskListMarker(checked) => {
                if let Some(item) = item_stack.last_mut() {
                    item.task = Some(*checked);
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
                    list.emit_tree(item, 0, out);
                }
            }
            Event::End(TagEnd::Paragraph) => {
                // A loose item's paragraphs stay on separate rows instead of
                // running together.
                if let Some(item) = item_stack.last_mut() {
                    item.inline.break_line();
                }
            }
            other => {
                if let Some(item) = item_stack.last_mut() {
                    item.inline.push_event(other);
                }
            }
        }
    }
    // Unclosed items (streaming safety): outermost first, so a partially
    // streamed list keeps source order.
    for (depth, item) in item_stack.into_iter().enumerate() {
        list.emit_tree(item, depth, out);
    }
}

impl ListRenderer<'_> {
    /// Emit an item (marker + inline-styled rows), then its children one level
    /// deeper — source order, glamour 2-column indent per level.
    fn emit_tree(&mut self, item: ItemBuf, depth: usize, out: &mut Vec<RenderLine>) {
        if !item.inline.is_empty() {
            self.emit_item(item.inline.finish(), item.task, item.number, depth, out);
        }
        for child in item.children {
            self.emit_tree(child, depth + 1, out);
        }
    }

    /// Render one list item: task checkbox or `◦`/numbered marker, the inline
    /// rows built from the item's own events, and a hanging indent. Over-wide
    /// rows are wrapped here, against the resolved content width, so every
    /// continuation row starts in the item's text column instead of falling
    /// back to the page edge.
    fn emit_item(
        &mut self,
        inlines: Vec<Line<'static>>,
        task: Option<bool>,
        number: Option<u64>,
        depth: usize,
        out: &mut Vec<RenderLine>,
    ) {
        let theme = self.theme;
        let indent = "  ".repeat(depth);
        let (marker, marker_style) = self.marker(task, number, depth);
        // Columns owned by the indent and marker: the continuation prefix and
        // the text budget are both derived from it, so the first row and every
        // wrapped row share one text column.
        let hang = indent.len() + UnicodeWidthStr::width(marker.as_str());
        let body_width = self
            .width
            .map(|width| width.saturating_sub(hang))
            .filter(|width| *width > 0);
        for (li, line) in inlines.into_iter().enumerate() {
            let rows = match body_width {
                Some(width) => wrap_styled_line(line, width),
                None => vec![line],
            };
            for (ri, row) in rows.into_iter().enumerate() {
                let mut spans = Vec::new();
                if li == 0 && ri == 0 {
                    spans.push(Span::styled(
                        indent.clone(),
                        theme.markdown.list_marker.style(),
                    ));
                    spans.push(Span::styled(marker.clone(), marker_style));
                } else {
                    // Continuation rows align under the text column.
                    spans.push(Span::styled(
                        " ".repeat(hang),
                        theme.markdown.list_marker.style(),
                    ));
                }
                spans.extend(row.spans);
                out.push(RenderLine {
                    line: Line::from(spans),
                    unit: self.unit,
                    // Wrapped rows stay on their logical row's source line.
                    raw_line: Some(self.raw_line_no),
                    atomic: false,
                    fill: false,
                });
            }
            self.raw_line_no += 1;
        }
    }

    /// Task checkbox, ordered number, or bullet marker. Top-level markers
    /// (unordered bullets and ordered numbers) render in the theme's `coral`
    /// tone; nested markers keep the regular muted `list_marker` tone.
    fn marker(&self, task: Option<bool>, number: Option<u64>, depth: usize) -> (String, Style) {
        let theme = self.theme;
        let top_level_style = if depth == 0 {
            Style::default().fg(theme.coral)
        } else {
            theme.markdown.list_marker.style()
        };
        match (task, number) {
            // glamour task: "[✓]" / "[ ]" followed by the item text.
            (Some(true), _) => ("[✓] ".to_string(), theme.markdown.task_checked.style()),
            (Some(false), _) => ("[ ] ".to_string(), theme.markdown.task_unchecked.style()),
            (None, Some(number)) => (format!("{number}. "), top_level_style),
            (None, None) => ("◦ ".to_string(), top_level_style),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn quick_links_render_before_wrap_and_preserve_complete_source() {
        let theme = Theme::ferra();
        let mut options = RenderOptions {
            content_width: Some(24),
            ..Default::default()
        };
        options.link_tags = vec![crate::link_copy::TaggedLink {
            target: "src/project/main.rs".into(),
            tag: '1',
        }];
        for source in [
            "see `src/project/main.rs`",
            "# src/project/main.rs",
            "- see src/project/main.rs",
            "> see src/project/main.rs",
            "```rust\nsrc/project/main.rs\n```",
            "| path |\n| --- |\n| src/project/main.rs |",
            "[source](src/project/main.rs)",
        ] {
            let mut units = HashMap::new();
            let rows = render_markdown(source, &theme, &mut 0, &options, &mut units);
            let text: String = rows
                .iter()
                .flat_map(|row| row.line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect();
            assert!(text.contains("~1"), "missing tag: {source}: {text}");
            assert!(
                units.values().any(|value| value.trim() == source),
                "source changed: {source}"
            );
        }
    }

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

    fn render_at(text: &str, width: usize) -> Vec<RenderLine> {
        let theme = Theme::ferra();
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            collapse_rows: 40,
            content_width: Some(width),
            ..Default::default()
        };
        render_markdown(text, &theme, &mut next, &options, &mut units)
    }

    #[test]
    fn nested_block_content_in_a_list_item_becomes_rows() {
        // A fenced code block inside an item arrives as text with embedded
        // newlines. A literal `\n` inside a span would corrupt the terminal
        // and the width math, so each source line becomes its own row and
        // picks up the item's hanging indent.
        let lines = render("- item\n\n  ```\n  code one\n  code two\n  ```\n");
        assert_eq!(plain(&lines), vec!["◦ item", "  code one", "  code two"]);
        assert!(
            lines.iter().all(|line| line
                .line
                .spans
                .iter()
                .all(|span| !span.content.contains('\n'))),
            "no span may carry a literal newline"
        );
    }

    #[test]
    fn table_renders_boxed_and_atomic() {
        let source = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
        for lines in [render(source), render_at(source, 24)] {
            let text = plain(&lines);
            assert!(
                text[0].starts_with("┌") && text[0].contains("┬"),
                "top border: {}",
                text[0]
            );
            assert_eq!(
                text.iter()
                    .filter_map(|row| row.chars().next())
                    .collect::<String>(),
                "┌│├│├│└",
                "one separator between logical rows: {text:?}"
            );
            assert!(lines.iter().all(|r| r.atomic), "all table rows atomic");
            assert!(
                lines.iter().all(|r| r.raw_line.is_none()),
                "no row-level mapping for table"
            );
        }
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
        assert_eq!(
            bold.style.add_modifier.contains(Modifier::BOLD),
            Theme::ferra().markdown.strong.bold
        );
        let code = lines
            .iter()
            .flat_map(|r| r.line.spans.iter())
            .find(|s| s.content == "code")
            .expect("code span rendered");
        let theme = Theme::ferra();
        assert_eq!(code.style.bg, theme.markdown.inline_code.bg);
        assert_eq!(code.style.fg, Some(theme.markdown.inline_code.fg));
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
    fn table_wraps_to_fit_width_limit() {
        let theme = Theme::ferra();
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            collapse_rows: 40,
            content_width: Some(24),
            ..Default::default()
        };
        let long = "x".repeat(30);
        let lines = render_markdown(
            &format!("| c |\n|---|\n| {long} |\n| next |"),
            &theme,
            &mut next,
            &options,
            &mut units,
        );
        let text = plain(&lines);
        for row in &text {
            assert!(
                UnicodeWidthStr::width(row.as_str()) <= 24,
                "table row exceeds limit: {row:?}"
            );
        }
        // One column with a 30-cell value must wrap inside the 20-cell column
        // instead of emitting an over-wide row that the outer layout would
        // later split and misalign.
        assert!(
            text.iter().any(|row| row.contains(&"x".repeat(20))),
            "first wrapped segment present: {text:?}"
        );
        assert!(
            text.iter().any(|row| row.trim_end().ends_with('\u{2502}')),
            "wrapped row closes its right border: {text:?}"
        );
        assert_eq!(
            UnicodeWidthStr::width(text[0].as_str()),
            24,
            "top border uses the limited width"
        );
        assert_eq!(text[0].chars().next(), Some('\u{250c}'));
        assert_eq!(text[0].chars().last(), Some('\u{2510}'));
        assert_eq!(
            text.iter()
                .filter_map(|row| row.chars().next())
                .collect::<String>(),
            "┌│├││├│└",
            "wrapped continuation lines stay together: {text:?}"
        );
    }

    #[test]
    fn table_collapsed_and_expanded_rows_have_separators() {
        let mut source = String::from("| step | description |\n|---|---|\n");
        for i in 1..=25 {
            source.push_str(&format!("| row{i:02} | operation details for this row |\n"));
        }
        let theme = Theme::ferra();
        for content_width in [None, Some(64)] {
            for expanded in [false, true] {
                let mut options = RenderOptions {
                    content_width,
                    ..Default::default()
                };
                if expanded {
                    options.expanded.insert(0);
                }
                let mut units = HashMap::new();
                let lines = render_markdown(&source, &theme, &mut 0, &options, &mut units);
                let text = plain(&lines);
                let expected_rows = if expanded { 26 } else { 7 };
                assert_eq!(text.len(), expected_rows * 2 + 1, "{text:?}");
                for (index, row) in text.iter().enumerate() {
                    let expected = if index == 0 {
                        '┌'
                    } else if index == text.len() - 1 {
                        '└'
                    } else if index % 2 == 0 {
                        '├'
                    } else {
                        '│'
                    };
                    assert_eq!(row.chars().next(), Some(expected), "{text:?}");
                }
                assert_eq!(
                    text.iter().any(|row| row.contains("20 lines hidden")),
                    !expanded
                );
                for i in 1..=25 {
                    assert_eq!(
                        text.iter().any(|row| row.contains(&format!("row{i:02}"))),
                        expanded || i <= 3 || i >= 24,
                        "retained row {i}: {text:?}"
                    );
                }
                assert!(lines
                    .iter()
                    .all(|row| row.unit == 0 && row.atomic && row.raw_line.is_none()));
                assert_eq!(units.get(&0).map(String::as_str), Some(source.as_str()));
            }
        }
    }

    #[test]
    fn table_uses_available_width_before_wrapping() {
        let theme = Theme::ferra();
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            collapse_rows: 40,
            content_width: Some(64),
            ..Default::default()
        };
        let long = "x".repeat(60);
        let lines = render_markdown(
            &format!("| c |\n|---|\n| {long} |"),
            &theme,
            &mut next,
            &options,
            &mut units,
        );
        let text = plain(&lines);
        // With a 64-column limit a single column can use 60 columns (border
        // overhead is 4), so the content should not be truncated at 40.
        assert!(
            text.iter().any(|row| row.contains(&"x".repeat(60))),
            "long single-column content uses the available width: {text:?}"
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
        let content = lines[1]
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(content, "  fn main() {}");
        let keyword = lines[1]
            .line
            .spans
            .iter()
            .find(|span| span.content == "fn")
            .expect("Rust keyword span");
        assert_eq!(keyword.style.fg, Some(Theme::ferra().code.keyword.fg));
        assert_eq!(keyword.style.bg, None, "token background stays transparent");
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
    fn streaming_fenced_code_keeps_the_current_unclosed_line() {
        let first = plain(&render("```rust\nfn main() {"));
        assert!(first.iter().any(|line| line == "  fn main() {"));

        let grown = plain(&render("```rust\nfn main() {\n    println!(\"hi\");"));
        assert!(grown.iter().any(|line| line == "  fn main() {"));
        assert!(grown.iter().any(|line| line.contains("println!")));

        let settled = plain(&render("```rust\nfn main() {\n    println!(\"hi\");\n```"));
        assert_eq!(
            grown, settled,
            "closing the fence must not repaint earlier rows"
        );
    }

    #[test]
    fn heading_strips_markers_and_uses_level_colors() {
        let lines = render("## 标题");
        let text = plain(&lines);
        assert_eq!(text[0], "标题", "glamour hides the # markers");
        let span = &lines[0].line.spans[0];
        let theme = Theme::ferra();
        assert_eq!(span.style.fg, Some(theme.markdown.heading2.fg));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
        // h1 keeps horizontal padding while its colors come entirely from
        // the semantic style (a theme may choose whether to define `bg`).
        let h1 = render("# 一级");
        let span = &h1[0].line.spans[0];
        assert_eq!(span.content, " ", "h1 padded with a leading space");
        assert_eq!(span.style.bg, theme.markdown.heading1.bg);
        assert_eq!(span.style.fg, Some(theme.markdown.heading1.fg));
        let h3 = render("### 三级");
        let span = &h3[0].line.spans[0];
        assert_eq!(span.style.fg, Some(theme.markdown.heading3.fg));
        assert_eq!(
            span.style.add_modifier.contains(Modifier::BOLD),
            theme.markdown.heading3.bold
        );
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
        assert_eq!(code.style.bg, Theme::ferra().markdown.inline_code.bg);
        let text: String = lines[0]
            .line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains(" cargo "), "padded spaces: {text}");
        let suffix = lines[0].line.spans.last().expect("plain suffix span");
        assert_eq!(suffix.content, " now");
        assert_eq!(
            suffix.style.bg, None,
            "source whitespace after inline code must not inherit chip bg"
        );
    }

    #[test]
    fn inline_code_respects_configured_padding_each_side() {
        let mut theme = Theme::ferra();
        theme.markdown.inline_code.padding = crate::theme::Padding::Separate { left: 2, right: 3 };
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            collapse_rows: 40,
            ..Default::default()
        };
        let lines = render_markdown("a `code` b", &theme, &mut next, &options, &mut units);
        let spans = &lines[0].line.spans;

        let code_style = theme.markdown.inline_code.style();
        // Spans: "a " (plain), "  " left pad, "code", "   " right pad, " b".
        let left_pad = spans
            .iter()
            .find(|s| s.content == "  " && s.style.bg == theme.markdown.inline_code.bg)
            .expect("left padding span");
        let right_pad = spans
            .iter()
            .find(|s| s.content == "   " && s.style.bg == theme.markdown.inline_code.bg)
            .expect("right padding span");
        assert_eq!(left_pad.style, code_style);
        assert_eq!(right_pad.style, code_style);
        let suffix = spans.last().expect("plain suffix").content.clone();
        assert_eq!(suffix, " b");
        assert_eq!(
            spans.last().unwrap().style.bg,
            None,
            "source separator must not inherit chip bg"
        );
    }

    #[test]
    fn inline_code_zero_padding_emits_no_backgrounded_spaces() {
        let mut theme = Theme::ferra();
        theme.markdown.inline_code.padding = crate::theme::Padding::All(0);
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            collapse_rows: 40,
            ..Default::default()
        };
        let lines = render_markdown("a `code` b", &theme, &mut next, &options, &mut units);
        let text: String = lines[0]
            .line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "a code b", "no padding spaces when padding is zero");
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
    fn over_wide_list_items_hang_under_their_text_column() {
        // Bullet marker = 2 columns, so a 12-column page wraps the text at 10
        // and every continuation row starts in the text column.
        let lines = render_at("- aaa bbb ccc ddd eee fff", 12);
        assert_eq!(plain(&lines), vec!["◦ aaa bbb", "  ccc ddd", "  eee fff"]);
        assert!(
            lines.iter().all(|line| line.line.width() <= 12),
            "no row exceeds the resolved content width"
        );
        assert!(
            lines.iter().all(|line| line.raw_line == Some(0)),
            "wrapped rows stay on their logical row's source line"
        );

        // The hanging indent follows the real marker width: `1. ` is 3 columns
        // and `[✓] ` is 4, not the fixed 2-column bullet indent.
        assert_eq!(
            plain(&render_at("1. aaa bbb ccc", 10)),
            vec!["1. aaa bbb", "   ccc"]
        );
        assert_eq!(
            plain(&render_at("- [x] aaa bbb ccc", 10)),
            vec!["[✓] aaa", "    bbb", "    ccc"]
        );
        // Nested items add their own 2-column level to the hanging indent.
        assert_eq!(
            plain(&render_at("- top\n  - nested aaa bbb", 10)),
            vec!["◦ top", "  ◦ nested", "    aaa", "    bbb"]
        );
        // Double-width text wraps on display cells, not character count.
        assert_eq!(
            plain(&render_at("- 一二三四五六", 10)),
            vec!["◦ 一二三四", "  五六"]
        );
    }

    #[test]
    fn list_items_keep_logical_rows_when_no_width_is_resolved() {
        // Without a resolved width the renderer emits unwrapped logical rows
        // and the paint-time wrapper stays authoritative.
        let lines = render("- aaa bbb ccc ddd eee fff");
        assert_eq!(plain(&lines), vec!["◦ aaa bbb ccc ddd eee fff"]);
        // A source soft break inside an item is a word separator, not a row:
        // the item owns its own wrapping.
        let lines = render("- first half\n  second half");
        assert_eq!(plain(&lines), vec!["◦ first half second half"]);
    }

    #[test]
    fn list_items_keep_escaped_markers_literal() {
        // A flattened re-parse consumed `1.` as an ordered marker and turned
        // escaped `\*` into emphasis; both must stay literal text.
        let lines = render("- 1\\. not a list");
        assert_eq!(
            plain(&lines),
            vec!["◦ 1. not a list"],
            "an escaped ordered marker keeps its digits"
        );
        let lines = render("- escaped \\*not em\\* here");
        assert_eq!(plain(&lines), vec!["◦ escaped *not em* here"]);
        assert!(
            lines[0]
                .line
                .spans
                .iter()
                .all(|span| !span.style.add_modifier.contains(Modifier::ITALIC)),
            "escaped asterisks must not become emphasis"
        );
    }

    #[test]
    fn loose_list_item_paragraphs_stay_on_separate_rows() {
        let lines = render("1. loose item\n\n   second paragraph");
        assert_eq!(plain(&lines), vec!["1. loose item", "   second paragraph"]);
        assert_eq!(lines[0].raw_line, Some(0));
        assert_eq!(lines[1].raw_line, Some(1));
    }

    #[test]
    fn ordered_numbers_follow_the_source_across_nesting() {
        // The source's own start number is authoritative.
        assert_eq!(
            plain(&render("3. three\n4. four")),
            vec!["3. three", "4. four"]
        );
        // Each level counts independently, and a nested list must not reset the
        // outer level back to bullets.
        assert_eq!(
            plain(&render(
                "1. one\n   1. nested one\n   2. nested two\n2. two"
            )),
            vec!["1. one", "  1. nested one", "  2. nested two", "2. two"]
        );
        assert_eq!(
            plain(&render("1. outer\n   - bullet child\n2. next")),
            vec!["1. outer", "  ◦ bullet child", "2. next"]
        );
    }

    #[test]
    fn ordered_list_numbers_per_level() {
        let lines = render("1. 甲\n2. 乙");
        let text = plain(&lines);
        assert_eq!(text[0], "1. 甲");
        assert_eq!(text[1], "2. 乙");
    }

    #[test]
    fn top_level_bullet_marker_is_coral_and_nested_keeps_muted_tone() {
        let theme = Theme::ferra();
        let lines = render("- top\n  - nested");
        let top = &lines[0].line.spans[1];
        assert!(top.content.starts_with('◦'), "top-level bullet marker");
        assert_eq!(
            top.style.fg,
            Some(theme.coral),
            "top-level bullet uses the coral tone"
        );
        let nested = &lines[1].line.spans[1];
        assert!(nested.content.starts_with('◦'), "nested bullet marker");
        assert_eq!(
            nested.style.fg,
            Some(theme.markdown.list_marker.fg),
            "nested bullet keeps the muted marker tone"
        );
        // Top-level ordered numbers use the coral tone too.
        let ordered = render("1. one");
        let top = &ordered[0].line.spans[1];
        assert!(top.content.starts_with("1."), "ordered marker");
        assert_eq!(
            top.style.fg,
            Some(theme.coral),
            "top-level ordered number uses the coral tone"
        );
    }

    #[test]
    fn nested_quote_stacks_bars() {
        let lines = render("> 外层\n> > 内层");
        let text = plain(&lines);
        assert_eq!(text[0], "│ 外层");
        assert_eq!(text[1], "│ │ 内层");
    }

    #[test]
    fn over_wide_quote_lines_repeat_their_bars_on_every_row() {
        let theme = Theme::ferra();
        // `│ ` is 2 columns, so a 12-column page wraps the text at 10 and the
        // gutter continues down every wrapped row.
        let lines = render_at("> aaa bbb ccc ddd", 12);
        assert_eq!(plain(&lines), vec!["│ aaa bbb", "│ ccc ddd"]);
        assert_eq!(
            lines[1].line.spans[0].style.fg,
            Some(theme.markdown.quote_marker.fg),
            "the wrapped row's bar keeps the quote marker tone"
        );
        assert!(
            lines.iter().all(|line| line.raw_line == Some(0)),
            "wrapped rows stay on their quote line's source line"
        );
        // Nested levels keep their own stack of bars on wrapped rows.
        assert_eq!(
            plain(&render_at("> > aaa bbb ccc", 10)),
            vec!["│ │ aaa", "│ │ bbb", "│ │ ccc"]
        );
    }

    #[test]
    fn code_block_gets_blank_separators() {
        let lines = render("前文\n\n```\ncode\n```\n\n后文");
        let text = plain(&lines);
        // Glow-style block: header row + content row, no frame.
        let header_idx = text
            .iter()
            .position(|l| l == "  code · 1 line")
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
        assert_eq!(
            bold.style.add_modifier.contains(Modifier::BOLD),
            Theme::ferra().markdown.strong.bold
        );
    }

    #[test]
    fn weak_markdown_routes_structure_and_code_through_weak_semantics() {
        let theme = Theme::ferra();
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            markdown_strength: MarkdownStrength::Weak,
            ..Default::default()
        };
        let lines = render_markdown(
            "## weak\n\n```rust\nfn main() {}\n```",
            &theme,
            &mut next,
            &options,
            &mut units,
        );
        let heading = lines
            .iter()
            .flat_map(|line| &line.line.spans)
            .find(|span| span.content == "weak")
            .expect("weak heading");
        assert_eq!(heading.style.fg, Some(theme.markdown_weak.heading2.fg));
        let keyword = lines
            .iter()
            .flat_map(|line| &line.line.spans)
            .find(|span| span.content == "fn")
            .expect("weak Rust keyword");
        assert_eq!(keyword.style.fg, Some(theme.code_weak.keyword.fg));
        assert_eq!(keyword.style.bg, None);
        assert!(lines
            .iter()
            .filter(|line| line.atomic)
            .all(|line| line.fill));
        assert_eq!(
            units.len(),
            2,
            "heading and code keep complete source units"
        );
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
            text.iter().any(|l| l.contains("80 lines hidden")),
            "collapse hint present: {:?}",
            text
        );
        assert!(lines.len() < 40, "collapsed: {} lines", lines.len());
    }

    #[test]
    fn chinese_code_block_uses_localized_chrome() {
        let mut code = String::from("```\n");
        for i in 0..100 {
            code.push_str(&format!("line {i}\n"));
        }
        code.push_str("```\n");
        let theme = Theme::ferra();
        let mut next = 0;
        let mut units = HashMap::new();
        let options = RenderOptions {
            language: Language::SimplifiedChinese,
            ..Default::default()
        };
        let lines = render_markdown(&code, &theme, &mut next, &options, &mut units);
        let text = plain(&lines);
        assert!(text.iter().any(|line| line.contains("代码 · 100 行")));
        assert!(text.iter().any(|line| line.contains("收起 80 行")));
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
            !text.iter().any(|l| l.contains("lines hidden")),
            "no hint when expanded"
        );
        // Glow layout: header row + 100 content rows + 1 bottom padding row.
        assert_eq!(lines.len(), 102);
    }
}
