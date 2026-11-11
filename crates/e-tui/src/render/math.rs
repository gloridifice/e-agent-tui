//! Bounded, fallible LaTeX parsing with character-grid layout only.

use pulldown_latex::{
    event::{
        Content, DimensionUnit, EnvironmentFlow, Event, Font, Grouping, ScriptPosition, ScriptType,
        StateChange, Visual,
    },
    Parser, Storage,
};
use rust_latex_parser::{EqNode, MathFontKind, MatrixKind};
use term_maths::RenderedBlock;
use unicode_width::UnicodeWidthStr;

const MAX_SOURCE_BYTES: usize = 1024;
const MAX_EVENTS: usize = 128;
const MAX_DEPTH: usize = 16;

pub(super) fn render_display(
    raw: &str,
    unit: u64,
    theme: &crate::config::Theme,
    options: &super::RenderOptions,
    out: &mut Vec<super::RenderLine>,
) -> bool {
    use pulldown_cmark::{Event as MdEvent, Options};
    use ratatui::text::{Line, Span};

    let source = super::math_source::MathSource::new(raw, Options::ENABLE_STRIKETHROUGH);
    if !source
        .events()
        .any(|(event, _)| matches!(event, MdEvent::DisplayMath(_)))
    {
        return false;
    }
    let mut builder = super::InlineBuilder::new(
        theme,
        theme.markdown.text.style(),
        super::SoftBreak::NewLine,
        &options.link_tags,
    );
    let mut lines = Vec::new();
    let mut images = std::collections::HashMap::new();
    for (event, range) in source.events() {
        if let MdEvent::DisplayMath(formula) = event {
            lines.extend(builder.take_lines());
            let rendered = render(&formula, true).map(|block| {
                if options
                    .content_width
                    .is_none_or(|width| block.width() <= width)
                {
                    images.insert(
                        lines.len(),
                        super::MathImage {
                            source: std::sync::Arc::from(formula.as_ref()),
                            width: block.width() as u16,
                            height: block.height() as u16,
                            foreground: theme.markdown.text.fg,
                        },
                    );
                }
                block.to_string()
            });
            let text = rendered.as_deref().unwrap_or(&raw[range]);
            for row in text.lines() {
                let mut line = Line::from(Span::styled(
                    if rendered.is_some() {
                        row
                    } else {
                        row.trim_end()
                    }
                    .to_owned(),
                    theme.markdown.text.style(),
                ));
                if rendered.is_some() {
                    if let Some(width) = options.content_width {
                        line = crate::wrap::clip_line(line, width);
                    }
                }
                lines.push(line);
            }
        } else if !matches!(event, MdEvent::SoftBreak | MdEvent::HardBreak) || !builder.is_empty() {
            builder.push_event(&event, &raw[range]);
        }
    }
    lines.extend(builder.take_lines());
    out.extend(
        lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| super::RenderLine {
                line,
                math_image: images.remove(&index),
                unit,
                raw_line: None,
                atomic: true,
                fill: false,
            }),
    );
    true
}

pub(super) fn render(source: &str, display: bool) -> Option<RenderedBlock> {
    if !admit(source) {
        return None;
    }
    let storage = Storage::new();
    let events = Parser::new(source, &storage)
        .take(MAX_EVENTS + 1)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if events.len() > MAX_EVENTS {
        return None;
    }
    let mut reader = MathReader {
        events: &events,
        cursor: 0,
        display,
    };
    let tree = reader.sequence(0, None)?;
    if reader.cursor != events.len() {
        return None;
    }
    let block = term_maths::layout::layout(&tree);
    if block.is_empty()
        || block.width() > 256
        || block.height() > 64
        || (!display && block.height() != 1)
    {
        return None;
    }
    Some(block)
}

fn admit(source: &str) -> bool {
    if source.is_empty() || source.len() > MAX_SOURCE_BYTES {
        return false;
    }
    let mut chars = source.chars().peekable();
    let mut depth = 0usize;
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let mut command = String::new();
                while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                    command.push(chars.next().unwrap());
                }
                if matches!(
                    command.as_str(),
                    "def" | "let" | "futurelet" | "newcommand" | "renewcommand" | "providecommand"
                ) {
                    return false;
                }
                if command.is_empty() {
                    chars.next();
                }
            }
            '{' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return false;
                }
            }
            '}' => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            // Comments can hide closing tokens; retain the source instead.
            '%' => return false,
            c if c.is_control() && !matches!(c, '\n' | '\r' | '\t') => return false,
            _ => {}
        }
    }
    depth == 0
}

struct MathReader<'a, 's> {
    events: &'a [Event<'s>],
    cursor: usize,
    display: bool,
}

impl MathReader<'_, '_> {
    fn sequence(&mut self, depth: usize, mut font: Option<MathFontKind>) -> Option<EqNode> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut nodes = Vec::new();
        while let Some(event) = self.events.get(self.cursor) {
            match event {
                Event::End | Event::EnvironmentFlow(_) => break,
                Event::StateChange(StateChange::Font(value)) => {
                    font = match value {
                        None | Some(Font::UpRight) => None,
                        Some(Font::Bold) => Some(MathFontKind::Bold),
                        Some(Font::DoubleStruck) => Some(MathFontKind::Blackboard),
                        Some(Font::Script) => Some(MathFontKind::Calligraphic),
                        Some(Font::Fraktur) => Some(MathFontKind::Fraktur),
                        Some(Font::Monospace) => Some(MathFontKind::Monospace),
                        Some(Font::SansSerif) => Some(MathFontKind::SansSerif),
                        _ => return None,
                    };
                    self.cursor += 1;
                }
                _ => nodes.push(self.element(depth + 1, font)?),
            }
        }
        Some(EqNode::Seq(nodes))
    }

    fn element(&mut self, depth: usize, font: Option<MathFontKind>) -> Option<EqNode> {
        if depth > MAX_DEPTH {
            return None;
        }
        let event = self.events.get(self.cursor)?.clone();
        self.cursor += 1;
        Some(match event {
            Event::Content(content) => {
                let text = match content {
                    Content::Text(s) | Content::Number(s) => s.to_owned(),
                    Content::Function(s) => format!("{s} "),
                    Content::Ordinary { content, .. }
                    | Content::LargeOp { content, .. }
                    | Content::Delimiter { content, .. }
                    | Content::Punctuation(content) => content.to_string(),
                    Content::BinaryOp { content, .. } => format!(" {content} "),
                    Content::Relation { content, .. } => {
                        let mut bytes = [0; 8];
                        let text =
                            std::str::from_utf8(content.encode_utf8_to_buf(&mut bytes)).ok()?;
                        format!(" {text} ")
                    }
                };
                if text.chars().any(char::is_control) || text.width() > 256 {
                    return None;
                }
                let node = EqNode::Text(text);
                match font {
                    Some(kind) => EqNode::MathFont {
                        kind,
                        content: Box::new(node),
                    },
                    None => node,
                }
            }
            Event::Begin(group) => {
                let node = match group {
                    Grouping::Normal => self.sequence(depth, font)?,
                    Grouping::LeftRight(left, right) => EqNode::Delimited {
                        left: delimiter(left)?,
                        right: delimiter(right)?,
                        content: Box::new(self.sequence(depth, font)?),
                    },
                    Grouping::Matrix {
                        alignment: pulldown_latex::event::ColumnAlignment::Center,
                    } => self.matrix(depth)?,
                    // Other environments have alignment or numbering semantics the grid does not implement.
                    _ => return None,
                };
                if self.events.get(self.cursor) != Some(&Event::End) {
                    return None;
                }
                self.cursor += 1;
                node
            }
            Event::Visual(Visual::Fraction(None)) => EqNode::Frac(
                Box::new(self.element(depth + 1, font)?),
                Box::new(self.element(depth + 1, font)?),
            ),
            Event::Visual(Visual::SquareRoot) => {
                EqNode::Sqrt(Box::new(self.element(depth + 1, font)?))
            }
            Event::Script { ty, position } => {
                let base = Box::new(self.element(depth + 1, font)?);
                let first = Box::new(self.element(depth + 1, font)?);
                let (lower, upper) = match ty {
                    ScriptType::Subscript => (Some(first), None),
                    ScriptType::Superscript => (None, Some(first)),
                    ScriptType::SubSuperscript => {
                        (Some(first), Some(Box::new(self.element(depth + 1, font)?)))
                    }
                };
                if position == ScriptPosition::AboveBelow
                    || (position == ScriptPosition::Movable && self.display)
                {
                    let mut node = *base;
                    if let Some(annotation) = lower {
                        node = EqNode::StackRel {
                            base: Box::new(node),
                            annotation,
                            over: false,
                        };
                    }
                    if let Some(annotation) = upper {
                        node = EqNode::StackRel {
                            base: Box::new(node),
                            annotation,
                            over: true,
                        };
                    }
                    node
                } else {
                    match (lower, upper) {
                        (Some(sub), Some(sup)) => EqNode::SupSub(base, sup, sub),
                        (Some(sub), None) => EqNode::Sub(base, sub),
                        (None, Some(sup)) => EqNode::Sup(base, sup),
                        (None, None) => return None,
                    }
                }
            }
            Event::Space {
                width,
                height: None,
            } => {
                let points = match width {
                    None => 0.0,
                    Some(d) => match d.unit {
                        DimensionUnit::Em => d.value * 18.0,
                        DimensionUnit::Mu | DimensionUnit::Pt => d.value,
                        _ => return None,
                    },
                };
                if !points.is_finite() || points.abs() > 36.0 {
                    return None;
                }
                EqNode::Space(points)
            }
            _ => return None,
        })
    }

    fn matrix(&mut self, depth: usize) -> Option<EqNode> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        loop {
            row.push(self.sequence(depth + 1, None)?);
            match self.events.get(self.cursor)? {
                Event::EnvironmentFlow(EnvironmentFlow::Alignment) => self.cursor += 1,
                Event::EnvironmentFlow(EnvironmentFlow::NewLine {
                    spacing: None,
                    horizontal_lines,
                }) if horizontal_lines.is_empty() => {
                    rows.push(std::mem::take(&mut row));
                    self.cursor += 1;
                    if self.events.get(self.cursor) == Some(&Event::End) {
                        break;
                    }
                }
                Event::End => {
                    rows.push(row);
                    break;
                }
                _ => return None,
            }
        }
        Some(EqNode::Matrix {
            kind: MatrixKind::Plain,
            rows,
        })
    }
}

fn delimiter(ch: Option<char>) -> Option<String> {
    match ch {
        None => Some(String::new()),
        Some(c @ ('(' | ')' | '[' | ']' | '{' | '}' | '|' | '‖' | '⟨' | '⟩')) => {
            Some(c.to_string())
        }
        _ => None,
    }
}
