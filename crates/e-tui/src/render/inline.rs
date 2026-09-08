//! Event-driven Markdown inline styling shared by block renderers.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::config::Theme;

/// Whether a source soft break starts a new logical row or is just a word
/// separator. Blocks that wrap for themselves (list items) must not turn a
/// source line break into a display row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SoftBreak {
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
pub(super) struct InlineBuilder<'a> {
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
    pub(super) fn new(theme: &'a Theme, base: Style, soft_break: SoftBreak) -> Self {
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

    pub(super) fn push_event(&mut self, event: &Event) {
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
    pub(super) fn break_line(&mut self) {
        self.lines.push(Line::default());
    }

    fn span_count(&self) -> usize {
        self.lines.last().map_or(0, |line| line.spans.len())
    }

    /// True while nothing but empty rows has been collected.
    pub(super) fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.width() == 0)
    }

    pub(super) fn finish(mut self) -> Vec<Line<'static>> {
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
pub(super) fn collect_inlines(theme: &Theme, raw: &str, base: Style) -> Vec<Line<'static>> {
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
