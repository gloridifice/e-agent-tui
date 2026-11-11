//! Preserve source offsets while exposing LaTeX delimiters to the Markdown parser.

use std::{borrow::Cow, ops::Range};

use pulldown_cmark::{Event, Options, Parser, Tag};

struct Region {
    range: Range<usize>,
    display: bool,
    closed: bool,
}

pub(super) struct MathSource<'a> {
    source: &'a str,
    masked: Cow<'a, str>,
    options: Options,
    regions: Vec<Region>,
}

impl<'a> MathSource<'a> {
    pub(super) fn new(source: &'a str, options: Options) -> Self {
        let options = options | Options::ENABLE_MATH;
        let mut result = Self {
            source,
            masked: Cow::Borrowed(source),
            options,
            regions: Vec::new(),
        };
        if !source.contains("\\(") && !source.contains("\\[") && !source.contains("$$") {
            return result;
        }
        let mut protected: Vec<Range<usize>> = Parser::new_ext(source, options)
            .into_offset_iter()
            .filter_map(|(event, range)| {
                matches!(
                    event,
                    Event::Code(_)
                        | Event::Html(_)
                        | Event::InlineHtml(_)
                        | Event::InlineMath(_)
                        | Event::DisplayMath(_)
                        | Event::Start(
                            Tag::CodeBlock(_)
                                | Tag::HtmlBlock
                                | Tag::Link { .. }
                                | Tag::Image { .. }
                        )
                )
                .then_some(range)
            })
            .collect();
        protected.sort_by_key(|range| range.start);
        let bytes = source.as_bytes();
        let mut index = 0;
        let mut protected_index = 0;
        while index + 1 < bytes.len() {
            while protected
                .get(protected_index)
                .is_some_and(|range| range.end <= index)
            {
                protected_index += 1;
            }
            if let Some(range) = protected.get(protected_index) {
                if range.start <= index {
                    index = range.end;
                    continue;
                }
            }
            let close = if bytes[index..].starts_with(b"$$") {
                b'$'
            } else if bytes[index] == b'\\' {
                match bytes[index + 1] {
                    b'(' => b')',
                    b'[' => b']',
                    _ => {
                        index += 2;
                        continue;
                    }
                }
            } else {
                index += 1;
                continue;
            };
            let limit = protected
                .get(protected_index)
                .map_or(bytes.len(), |range| range.start);
            let mut end = index + 2;
            let mut closed = false;
            while end < limit {
                if bytes[end..limit].starts_with(b"\n\n")
                    || bytes[end..limit].starts_with(b"\r\n\r\n")
                {
                    break;
                }
                if close == b'$' && bytes[end..limit].starts_with(b"$$") {
                    end += 2;
                    closed = true;
                    break;
                }
                if bytes[end] == b'\\' && end + 1 < limit {
                    if close != b'$' && bytes[end + 1] == close {
                        end += 2;
                        closed = true;
                        break;
                    }
                    end += 2;
                } else {
                    end += 1;
                }
            }
            if end - index > 4 {
                result.regions.push(Region {
                    range: index..end,
                    display: close != b')',
                    closed,
                });
            }
            index = end;
        }
        if !result.regions.is_empty() {
            let mut masked = source.as_bytes().to_vec();
            for region in &result.regions {
                // A same-length opaque math span prevents Markdown from consuming TeX escapes,
                // braces, and underscores. All emitted content comes from the original source.
                masked[region.range.clone()].fill(b'x');
                masked[region.range.start..region.range.start + 2].fill(b'$');
                masked[region.range.end - 2..region.range.end].fill(b'$');
            }
            result.masked =
                Cow::Owned(String::from_utf8(masked).expect("whole UTF-8 regions replaced"));
            let matched = Parser::new_ext(&result.masked, options)
                .into_offset_iter()
                .filter(|(event, range)| {
                    matches!(event, Event::DisplayMath(_))
                        && result
                            .regions
                            .binary_search_by_key(&range.start, |region| region.range.start)
                            .is_ok_and(|index| result.regions[index].range == *range)
                })
                .count();
            if matched != result.regions.len() {
                result.masked = Cow::Borrowed(source);
                result.regions.clear();
            }
        }
        result
    }

    pub(super) fn events(&self) -> impl Iterator<Item = (Event<'_>, Range<usize>)> {
        Parser::new_ext(&self.masked, self.options)
            .into_offset_iter()
            .map(|(event, mut range)| {
                let region = self
                    .regions
                    .binary_search_by_key(&range.start, |region| region.range.start)
                    .ok()
                    .map(|index| &self.regions[index])
                    .filter(|region| region.range == range);
                let event = match (event, region) {
                    (Event::DisplayMath(_), Some(region)) => {
                        if !region.closed {
                            Event::Text(self.source[range.clone()].into())
                        } else {
                            let formula = &self.source[range.start + 2..range.end - 2];
                            if region.display {
                                Event::DisplayMath(formula.into())
                            } else {
                                Event::InlineMath(formula.into())
                            }
                        }
                    }
                    (Event::Text(_), _)
                        if self.source[range.clone()].starts_with(['[', '(', ']', ')'])
                            && self.source.as_bytes()[..range.start]
                                .iter()
                                .rev()
                                .take_while(|byte| **byte == b'\\')
                                .count()
                                % 2
                                == 1 =>
                    {
                        range.start -= 1;
                        Event::Text(self.source[range.clone()].into())
                    }
                    (event, _) => event,
                };
                (event, range)
            })
    }
}
