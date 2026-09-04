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
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    config::Theme,
    i18n::{tr_args, Language},
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
            let inlines = collect_inlines(theme, raw, theme.markdown.text.style());
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
        let inlines = collect_inlines(theme, content, theme.markdown.text.style());
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

/// Whether a source soft break starts a new logical row or is just a word
/// separator. Blocks that wrap for themselves (list items) must not turn a
/// source line break into a display row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SoftBreak {
    NewLine,
    Space,
}

/// Event-driven inline styling shared by paragraphs, headings, quotes, table
/// cells, and list items. Inline styling follows glamour dark: strong/emph turn
/// pink, inline code is a padded pink-on-soft chip, links render `text` (bold
/// pink) followed by the underlined URL, and a style stack keeps nesting
/// correct.
///
/// Callers feed the events of the block they are already parsing. Never
/// re-parse a block's flattened text: the parser has consumed the markers by
/// then, so chips, emphasis, link URLs, and escapes silently disappear and a
/// leading `3.`/`#` in the text is eaten as a block marker.
struct InlineBuilder<'a> {
    theme: &'a Theme,
    base: Style,
    soft_break: SoftBreak,
    lines: Vec<Line<'static>>,
    style: Style,
    stack: Vec<Style>,
    /// Pending link/image: (url, span index on the current line at start).
    pending: Option<(String, usize)>,
}

impl<'a> InlineBuilder<'a> {
    fn new(theme: &'a Theme, base: Style, soft_break: SoftBreak) -> Self {
        Self {
            theme,
            base,
            soft_break,
            lines: vec![Line::default()],
            style: base,
            stack: Vec::new(),
            pending: None,
        }
    }

    fn push_event(&mut self, event: &Event) {
        match event {
            Event::Text(text) => self.push_text(text),
            Event::Code(code) => {
                // Inline code owns an independent semantic foreground/background.
                // Padding is configurable per side via the theme's inline_code
                // semantic style; it renders as backgrounded spaces so the chip
                // reads as a padded block.
                let inline = self.theme.markdown.inline_code;
                let code_style = inline.style();
                let left = inline.padding.left();
                let right = inline.padding.right();
                let line = self.lines.last_mut().unwrap();
                if left > 0 {
                    line.push_span(Span::styled(" ".repeat(left), code_style));
                }
                line.push_span(Span::styled(code.to_string(), code_style));
                if right > 0 {
                    line.push_span(Span::styled(" ".repeat(right), code_style));
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => self.push_text(html),
            Event::SoftBreak => match self.soft_break {
                SoftBreak::NewLine => self.break_line(),
                SoftBreak::Space => self.push_text(" "),
            },
            Event::HardBreak => self.break_line(),
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: &Tag) {
        match tag {
            Tag::Emphasis => {
                self.stack.push(self.style);
                self.style = self.style.patch(self.theme.markdown.emphasis.style());
            }
            Tag::Strong => {
                self.stack.push(self.style);
                self.style = self.style.patch(self.theme.markdown.strong.style());
            }
            Tag::Strikethrough => {
                self.stack.push(self.style);
                self.style = self
                    .style
                    .patch(self.theme.markdown.strikethrough.style())
                    .add_modifier(Modifier::CROSSED_OUT);
            }
            Tag::Link { dest_url, .. } => {
                self.stack.push(self.style);
                self.pending = Some((dest_url.to_string(), self.span_count()));
                self.style = self.style.patch(self.theme.markdown.link_text.style());
            }
            Tag::Image { dest_url, .. } => {
                self.stack.push(self.style);
                self.pending = Some((dest_url.to_string(), self.span_count()));
                self.style = self.style.patch(self.theme.markdown.image.style());
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: &TagEnd) {
        match tag {
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.style = self.stack.pop().unwrap_or(self.base);
            }
            TagEnd::Link | TagEnd::Image => {
                // glamour renders the URL after the link text, underlined.
                if let Some((url, start_idx)) = self.pending.take() {
                    if !url.is_empty() {
                        let spans = &self.lines.last().unwrap().spans;
                        let text: String = spans[start_idx.min(spans.len())..]
                            .iter()
                            .map(|s| s.content.as_ref())
                            .collect();
                        if text != url {
                            let url_style = self.theme.markdown.link_url.style();
                            let line = self.lines.last_mut().unwrap();
                            line.push_span(Span::styled(" ", self.base));
                            line.push_span(Span::styled(url, url_style));
                        }
                    }
                }
                self.style = self.stack.pop().unwrap_or(self.base);
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        // Embedded newlines arrive with code or HTML blocks nested inside a
        // list item: they are row breaks, never literal glyphs in a span.
        let mut segments = text.split('\n');
        if let Some(first) = segments.next() {
            self.push_segment(first);
        }
        for segment in segments {
            self.break_line();
            self.push_segment(segment);
        }
    }

    fn push_segment(&mut self, text: &str) {
        // A stray carriage return would move the terminal cursor, so drop it
        // with the row break that produced it.
        let text = text.trim_end_matches('\r');
        if text.is_empty() {
            return;
        }
        let span = Span::styled(text.to_string(), self.style);
        self.lines.last_mut().unwrap().push_span(span);
    }

    /// Start a new logical row (hard break, or a loose list item's next
    /// paragraph).
    fn break_line(&mut self) {
        self.lines.push(Line::default());
    }

    fn span_count(&self) -> usize {
        self.lines.last().map_or(0, |line| line.spans.len())
    }

    /// True while nothing but empty rows has been collected.
    fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.width() == 0)
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        for line in self.lines.iter_mut() {
            trim_line_end(line);
        }
        // Drop trailing empty line artifacts.
        while self.lines.last().is_some_and(|line| line.width() == 0) {
            self.lines.pop();
        }
        if self.lines.is_empty() {
            self.lines.push(Line::default());
        }
        self.lines
    }
}

/// Collect inline content of a paragraph/heading/quote/table cell from its own
/// source. Soft and hard breaks split rows, matching the raw source's own line
/// breaks.
fn collect_inlines(theme: &Theme, raw: &str, base: Style) -> Vec<Line<'static>> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut builder = InlineBuilder::new(theme, base, SoftBreak::NewLine);
    for event in Parser::new_ext(raw, options) {
        builder.push_event(&event);
    }
    builder.finish()
}

/// Drop trailing plain whitespace so a stray source space cannot wrap into an
/// extra row. Whitespace carrying its own background (inline-code chip
/// padding) is content and must survive.
fn trim_line_end(line: &mut Line<'static>) {
    while let Some(last) = line.spans.last() {
        if last.style.bg.is_some() {
            break;
        }
        let trimmed = last.content.trim_end();
        if trimmed.is_empty() {
            line.spans.pop();
            continue;
        }
        if trimmed.len() != last.content.len() {
            let style = last.style;
            let text = trimmed.to_string();
            line.spans.pop();
            line.spans.push(Span::styled(text, style));
        }
        break;
    }
}

// ---------------------------------------------------------------------------
// Mermaid (D9): grok-mermaid WASM -> styled box art; trap falls back to the
// fenced source. The block stays atomic: copy mode yields the raw mermaid.
// ---------------------------------------------------------------------------

fn fenced_content<'a>(raw_lines: &'a [&'a str]) -> (&'a [&'a str], usize) {
    let Some(opening) = raw_lines.first().map(|line| line.trim_start()) else {
        return (&[], 0);
    };
    let Some(marker) = opening
        .chars()
        .next()
        .filter(|marker| matches!(marker, '`' | '~'))
    else {
        return (raw_lines, 0);
    };
    let fence_len = opening
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if fence_len < 3 {
        return (raw_lines, 0);
    }
    let has_closing_fence = raw_lines.last().is_some_and(|line| {
        let closing = line.trim();
        closing
            .chars()
            .take_while(|character| *character == marker)
            .count()
            >= fence_len
            && closing.trim_start_matches(marker).trim().is_empty()
    });
    let end = raw_lines
        .len()
        .saturating_sub(usize::from(has_closing_fence));
    (&raw_lines[1.min(end)..end], 1)
}

fn render_mermaid_block(
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let (content, _) = fenced_content(&raw_lines);
    let source = content.join("\n");
    let dim = theme.code.meta.style();
    let source_lines = source.lines().count();
    // Glow-style header: `  mermaid · N lines` on the filled block.
    out.push(RenderLine {
        line: Line::from(vec![
            Span::styled("  ", dim),
            Span::styled("mermaid", dim),
            Span::styled(
                tr_args(
                    options.language,
                    if source_lines == 1 {
                        "markdown.block_line"
                    } else {
                        "markdown.block_lines"
                    },
                    &[("count", source_lines.to_string())],
                ),
                dim,
            ),
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
                            crate::mermaid::MermaidClass::Border => {
                                theme.markdown.mermaid_border.style()
                            }
                            crate::mermaid::MermaidClass::Node => {
                                theme.markdown.mermaid_node.style()
                            }
                            crate::mermaid::MermaidClass::Edge => {
                                theme.markdown.mermaid_edge.style()
                            }
                            crate::mermaid::MermaidClass::EdgeLabel => {
                                theme.markdown.mermaid_edge_label.style()
                            }
                            crate::mermaid::MermaidClass::Title => {
                                theme.markdown.mermaid_title.style()
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
                out.push(collapse_hint_row(unit, theme, options.language, hidden));
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
                        tr_args(
                            options.language,
                            "markdown.mermaid_error",
                            &[("error", error.to_string())],
                        ),
                        theme.code.meta.style(),
                    ),
                ]),
                unit,
                raw_line: None,
                atomic: true,
                fill: true,
            });
            for (i, line) in content.iter().enumerate() {
                out.push(RenderLine {
                    line: Line::from(vec![
                        Span::styled("  ", dim),
                        Span::styled((*line).to_string(), theme.code.text.style()),
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
    fenced: bool,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let (content, fence_offset) = if fenced {
        fenced_content(&raw_lines)
    } else {
        (raw_lines.as_slice(), 0)
    };
    let dim = theme.code.meta.style();
    let language_key = if content.len() == 1 {
        "markdown.block_line"
    } else {
        "markdown.block_lines"
    };
    // Glow-style header: `  lang · N lines` — no frame.
    out.push(RenderLine {
        line: Line::from(vec![
            Span::styled("  ", dim),
            Span::styled(
                lang.map_or_else(
                    || crate::i18n::tr(options.language, "markdown.code"),
                    str::to_owned,
                ),
                dim,
            ),
            Span::styled(
                tr_args(
                    options.language,
                    language_key,
                    &[("count", content.len().to_string())],
                ),
                dim,
            ),
        ]),
        unit,
        raw_line: Some(0),
        atomic: true,
        fill: true,
    });

    let highlighted = crate::syntax::highlight_lines(
        content,
        crate::syntax::SyntaxHint::Token(lang.unwrap_or_default()),
        &theme.code,
    );
    let collapsed = !options.expanded.contains(&unit) && content.len() > options.collapse_rows;
    let push_content = |i: usize, out: &mut Vec<RenderLine>| {
        let mut spans = vec![Span::styled("  ", dim)];
        spans.extend(highlighted[i].spans.clone());
        out.push(RenderLine {
            line: Line::from(spans),
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
        out.push(collapse_hint_row(unit, theme, options.language, hidden));
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
fn collapse_hint_row(unit: u64, theme: &Theme, language: Language, hidden: usize) -> RenderLine {
    RenderLine {
        line: Line::from(Span::styled(
            tr_args(
                language,
                "markdown.collapse_hint",
                &[("hidden", hidden.to_string())],
            ),
            theme.code.meta.style(),
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
        line: Line::from(Span::styled(" ", theme.markdown.code_block_bg.style())),
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
fn wrap_styled_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    crate::wrap::wrap_line(line, width)
}
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
            &format!("| c |\n|---|\n| {long} |"),
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
            text.iter().any(|l| l.contains("[Enter expand]")),
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
        assert!(text
            .iter()
            .any(|line| line.contains("收起 80 行 [Enter 展开]")));
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
            !text.iter().any(|l| l.contains("[Enter expand]")),
            "no hint when expanded"
        );
        // Glow layout: header row + 100 content rows + 1 bottom padding row.
        assert_eq!(lines.len(), 102);
    }
}
