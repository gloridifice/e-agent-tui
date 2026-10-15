//! Frontend input aliases, header parsing, and bounded display previews.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptMacro {
    pub id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<PromptMacro>", into = "Vec<PromptMacro>")]
pub struct PromptMacros(Vec<PromptMacro>);

pub fn valid_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes.next().is_some_and(|c| c.is_ascii_alphabetic())
        && bytes.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}

impl TryFrom<Vec<PromptMacro>> for PromptMacros {
    type Error = String;

    fn try_from(entries: Vec<PromptMacro>) -> Result<Self, Self::Error> {
        for (index, entry) in entries.iter().enumerate() {
            if !valid_id(&entry.id) || entry.prompt.trim().is_empty() {
                return Err("prompt macro requires a valid ID and nonempty body".into());
            }
            if entry
                .model
                .iter()
                .chain(entry.effort.iter())
                .any(|value| value.is_empty() || value.chars().any(char::is_whitespace))
            {
                return Err("prompt macro model and effort must be nonempty identifiers".into());
            }
            if entries[..index].iter().any(|other| other.id == entry.id) {
                return Err(format!("duplicate prompt macro: {}", entry.id));
            }
        }
        Ok(Self(entries))
    }
}

impl From<PromptMacros> for Vec<PromptMacro> {
    fn from(macros: PromptMacros) -> Self {
        macros.0
    }
}

impl PromptMacros {
    pub fn entries(&self) -> &[PromptMacro] {
        &self.0
    }

    pub fn exact_match(&self, raw: &str, has_images: bool) -> Option<&PromptMacro> {
        (!has_images)
            .then(|| {
                self.0
                    .iter()
                    .find(|entry| entry.id == raw.trim_end_matches(' '))
            })
            .flatten()
    }

    pub fn set(&mut self, entry: PromptMacro) {
        if let Some(existing) = self.0.iter_mut().find(|other| other.id == entry.id) {
            *existing = entry;
        } else {
            self.0.push(entry);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    Id,
    Option,
    Model,
    Effort,
    Body,
}

pub struct HeaderContext<'a> {
    pub kind: HeaderKind,
    pub range: Range<usize>,
    pub query: &'a str,
    pub model: Option<&'a str>,
    pub has_model: bool,
    pub has_effort: bool,
}

struct Header<'a> {
    fields: Vec<(HeaderKind, Range<usize>)>,
    id: &'a str,
    model: Option<&'a str>,
    effort: Option<&'a str>,
    has_model: bool,
    has_effort: bool,
    body: usize,
    next: HeaderKind,
    error: Option<String>,
}

fn token(line: &str, offset: &mut usize) -> Option<Range<usize>> {
    *offset += line[*offset..].len() - line[*offset..].trim_start().len();
    if *offset == line.len() {
        return None;
    }
    let start = *offset;
    *offset += line[start..]
        .find(char::is_whitespace)
        .unwrap_or(line.len() - start);
    Some(start..*offset)
}

fn header(line: &str) -> Option<Header<'_>> {
    let rest = line.strip_prefix("/prompt-macro")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut offset = line.len() - rest.len();
    let mut result = Header {
        fields: Vec::new(),
        id: "",
        model: None,
        effort: None,
        has_model: false,
        has_effort: false,
        body: line.len(),
        next: HeaderKind::Id,
        error: None,
    };
    let Some(id) = token(line, &mut offset) else {
        return Some(result);
    };
    result.id = &line[id.clone()];
    result.fields.push((HeaderKind::Id, id));
    result.next = HeaderKind::Option;
    while let Some(range) = token(line, &mut offset) {
        let value = &line[range.clone()];
        if value == "--" {
            result.fields.push((HeaderKind::Option, range));
            result.body = offset + line[offset..].len() - line[offset..].trim_start().len();
            result.next = HeaderKind::Body;
            break;
        }
        let kind = match value {
            "--model" => HeaderKind::Model,
            "--effort" => HeaderKind::Effort,
            value if value.starts_with("--") => {
                result.fields.push((HeaderKind::Option, range));
                result.error = Some(format!("unknown macro option: {value}"));
                break;
            }
            _ => {
                result.body = range.start;
                result.next = HeaderKind::Body;
                break;
            }
        };
        result.fields.push((HeaderKind::Option, range));
        let seen = if kind == HeaderKind::Model {
            &mut result.has_model
        } else {
            &mut result.has_effort
        };
        if *seen {
            result.error = Some(format!("duplicate macro option: {value}"));
        }
        *seen = true;
        result.next = kind;
        let Some(argument) = token(line, &mut offset) else {
            break;
        };
        let value = &line[argument.clone()];
        if value.starts_with("--") {
            result.error = Some("macro option requires a value".into());
            break;
        }
        if kind == HeaderKind::Model {
            result.model = Some(value)
        } else {
            result.effort = Some(value)
        }
        result.fields.push((kind, argument));
        result.next = HeaderKind::Option;
    }
    Some(result)
}

pub fn parse_definition(line: &str) -> Result<PromptMacro, String> {
    let parsed = header(line).ok_or("expected /prompt-macro")?;
    if let Some(error) = parsed.error {
        return Err(error);
    }
    if !valid_id(parsed.id) {
        return Err("macro ID must match [A-Za-z][A-Za-z0-9_-]*".into());
    }
    if parsed.has_model && parsed.model.is_none() || parsed.has_effort && parsed.effort.is_none() {
        return Err("macro option requires a value".into());
    }
    let prompt = &line[parsed.body..];
    if prompt.trim().is_empty() {
        return Err("macro body must not be empty".into());
    }
    Ok(PromptMacro {
        id: parsed.id.into(),
        prompt: prompt.into(),
        model: parsed.model.map(str::to_owned),
        effort: parsed.effort.map(str::to_owned),
    })
}

pub fn definition_context(line: &str, cursor: usize) -> Option<HeaderContext<'_>> {
    let parsed = header(line)?;
    if cursor < "/prompt-macro".len() || !line.is_char_boundary(cursor) {
        return None;
    }
    let (kind, range) = if let Some((kind, range)) = parsed
        .fields
        .iter()
        .find(|(_, range)| range.start <= cursor && cursor <= range.end)
    {
        (*kind, range.start..cursor)
    } else if parsed.next == HeaderKind::Body && cursor >= parsed.body {
        (HeaderKind::Body, parsed.body..cursor)
    } else {
        let previous = parsed
            .fields
            .iter()
            .rev()
            .find(|(_, range)| range.end < cursor);
        let kind = match previous {
            None => HeaderKind::Id,
            Some((HeaderKind::Option, range)) => match &line[range.clone()] {
                "--model" => HeaderKind::Model,
                "--effort" => HeaderKind::Effort,
                "--" => HeaderKind::Body,
                _ => HeaderKind::Option,
            },
            _ => HeaderKind::Option,
        };
        (kind, cursor..cursor)
    };
    Some(HeaderContext {
        kind,
        query: &line[range.clone()],
        range,
        model: parsed.model,
        has_model: parsed.has_model,
        has_effort: parsed.has_effort,
    })
}

pub fn preview(body: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut ends = Vec::new();
    let mut used = 0;
    let mut truncated = false;
    for grapheme in body.graphemes(true) {
        if grapheme.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            truncated = true;
            break;
        }
        let safe =
            if grapheme.chars().any(char::is_control) || UnicodeWidthStr::width(grapheme) == 0 {
                "�"
            } else {
                grapheme
            };
        let cells = UnicodeWidthStr::width(safe);
        if used + cells > width {
            truncated = true;
            break;
        }
        ends.push((output.len(), used));
        output.push_str(safe);
        used += cells;
    }
    if truncated {
        while used + UnicodeWidthStr::width("…") > width {
            let Some((bytes, cells)) = ends.pop() else {
                break;
            };
            output.truncate(bytes);
            used = cells;
        }
        output.push('…');
    }
    output
}
