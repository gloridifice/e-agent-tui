//! Bounded, display-only preparation of direct Python/Node stdin scripts.

use std::{ops::Range, path::Path, sync::OnceLock};

use dprint_plugin_typescript::{
    configuration::{Configuration, ConfigurationBuilder},
    format_text, FormatTextOptions,
};
use ruff_python_formatter::{format_module_source, PyFormatOptions, QuoteStyle};

const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 4 * 1024;
const MAX_FORMAT_BYTES: usize = 16 * 1024;
const MAX_FORMAT_LINES: usize = 400;
const MAX_FORMAT_LINE_BYTES: usize = 4 * 1024;

#[derive(Debug, Default)]
pub(crate) struct ScriptCache {
    source: String,
    script: Option<Script>,
}

impl ScriptCache {
    pub(crate) fn get(&mut self, source: &str) -> Option<&Script> {
        if source.len() > MAX_SOURCE_BYTES {
            *self = Self::default();
            return None;
        }
        if self.source != source {
            self.script = prepare(source);
            self.source = source.to_owned();
        }
        self.script.as_ref()
    }
}

#[derive(Debug)]
pub(crate) struct Script {
    pub body: Range<usize>,
    pub formatted: Option<String>,
}

#[derive(Clone, Copy)]
enum Language {
    Python,
    JavaScript,
}

struct Word {
    value: String,
    quoted: bool,
    expands: bool,
}

struct Header<'a> {
    rest: &'a str,
}

impl Header<'_> {
    fn next_char(&mut self) -> Option<char> {
        let ch = self.rest.chars().next()?;
        self.rest = &self.rest[ch.len_utf8()..];
        Some(ch)
    }

    fn skip_space(&mut self) {
        loop {
            self.rest = self.rest.trim_start_matches([' ', '\t', '\r']);
            if let Some(rest) = self
                .rest
                .strip_prefix("\\\n")
                .or_else(|| self.rest.strip_prefix("\\\r\n"))
            {
                self.rest = rest;
            } else {
                break;
            }
        }
    }

    fn word(&mut self) -> Option<Word> {
        let start = self.rest.len();
        let mut value = String::new();
        let mut quoted = false;
        let mut expands = false;
        let mut quote = None;
        while let Some(ch) = self.rest.chars().next() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            if quote.is_none() && (ch.is_whitespace() || "<>;&|()".contains(ch)) {
                break;
            }
            self.next_char();
            if quote.is_none() && ch == '$' && self.rest.starts_with(['\'', '"']) {
                return None;
            }
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() && matches!(ch, '\'' | '"') {
                quoted = true;
                quote = Some(ch);
            } else if ch == '\\' && quote != Some('\'') {
                let next = self.next_char()?;
                if next == '\n' {
                    continue;
                }
                if next == '\r' && self.rest.starts_with('\n') {
                    self.next_char();
                    continue;
                }
                if quote == Some('"') && !"$`\"\\".contains(next) {
                    value.push('\\');
                }
                quoted = true;
                value.push(next);
            } else {
                expands |= quote != Some('\'') && matches!(ch, '$' | '`');
                expands |= quote.is_none() && matches!(ch, '*' | '?' | '[' | '~' | '{' | '}');
                value.push(ch);
            }
        }
        (self.rest.len() < start && quote.is_none()).then_some(Word {
            value,
            quoted,
            expands,
        })
    }
}

fn prepare(source: &str) -> Option<Script> {
    let mut header = Header { rest: source };
    let mut words = Vec::new();
    let mut heredoc = None;
    loop {
        header.skip_space();
        if source.len() - header.rest.len() > MAX_HEADER_BYTES {
            return None;
        }
        if header.rest.starts_with('#') {
            header.rest = &header.rest[header.rest.find('\n')?..];
        }
        if header.rest.starts_with('\n') {
            header.next_char();
            break;
        }
        let redirect = header
            .rest
            .strip_prefix("0<<")
            .or_else(|| header.rest.strip_prefix("<<"));
        if let Some(rest) = redirect {
            if heredoc.is_some() {
                return None;
            }
            header.rest = rest;
            let strip_tabs = header.rest.starts_with('-');
            if strip_tabs {
                header.next_char();
            }
            header.skip_space();
            let delimiter = header.word()?;
            heredoc = Some((delimiter, strip_tabs));
        } else {
            let digits = header.rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && header.rest[digits..].starts_with('<') {
                return None;
            }
            let word = header.word()?;
            if word.expands {
                return None;
            }
            words.push(word.value);
        }
    }
    let language = stdin_language(&words)?;
    let (delimiter, strip_tabs) = heredoc?;
    let start = source.len() - header.rest.len();
    if start > MAX_HEADER_BYTES {
        return None;
    }
    let mut end = start;
    let mut complete = false;
    for line in header.rest.split_inclusive('\n') {
        // Unquoted heredoc continuation changes delimiter recognition.
        if !delimiter.quoted && (line.ends_with("\\\n") || line.ends_with("\\\r\n")) {
            return None;
        }
        let logical = line.strip_suffix('\n').unwrap_or(line);
        let logical = logical.strip_suffix('\r').unwrap_or(logical);
        let logical = if strip_tabs {
            logical.trim_start_matches('\t')
        } else {
            logical
        };
        if logical == delimiter.value {
            if !source[end + line.len()..].trim().is_empty() {
                return None;
            }
            complete = true;
            break;
        }
        end += line.len();
    }
    let body = &source[start..end];
    let formatted = if complete && delimiter.quoted {
        let code = if strip_tabs {
            body.split_inclusive('\n')
                .map(|line| line.trim_start_matches('\t'))
                .collect::<String>()
        } else {
            body.to_owned()
        };
        format(&code, language)
    } else {
        None
    };
    Some(Script {
        body: start..end,
        formatted,
    })
}

fn stdin_language(words: &[String]) -> Option<Language> {
    let executable = words.first()?.rsplit(['/', '\\']).next()?;
    let executable = executable.strip_suffix(".exe").unwrap_or(executable);
    let language = if executable == "python"
        || executable.strip_prefix("python").is_some_and(|version| {
            !version.is_empty()
                && version
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        }) {
        Language::Python
    } else if matches!(executable, "node" | "nodejs") {
        Language::JavaScript
    } else {
        return None;
    };
    let mut args = words[1..].iter().map(String::as_str);
    while let Some(arg) = args.next() {
        match arg {
            "-" => return Some(language),
            "--" => return args.next().is_none_or(|arg| arg == "-").then_some(language),
            _ => {}
        }
        let accepted = match language {
            Language::Python => {
                if matches!(arg, "-W" | "-X") {
                    args.next()?;
                    true
                } else {
                    arg.strip_prefix('-').is_some_and(|flags| {
                        !flags.is_empty() && flags.chars().all(|ch| "bBEiIOqsSuv".contains(ch))
                    }) || ((arg.starts_with("-W") || arg.starts_with("-X")) && arg.len() > 2)
                }
            }
            Language::JavaScript => match arg {
                "--input-type" => matches!(args.next(), Some("module" | "commonjs")),
                "-r" | "--require" | "--import" => {
                    args.next()?;
                    true
                }
                "--input-type=module"
                | "--input-type=commonjs"
                | "--no-warnings"
                | "--trace-warnings"
                | "--enable-source-maps"
                | "--no-deprecation"
                | "--trace-deprecation"
                | "--check"
                | "-c" => true,
                _ => false,
            },
        };
        if !accepted {
            return None;
        }
    }
    Some(language)
}

fn format(source: &str, language: Language) -> Option<String> {
    if source.len() > MAX_FORMAT_BYTES
        || source.lines().count() > MAX_FORMAT_LINES
        || source
            .lines()
            .any(|line| line.len() > MAX_FORMAT_LINE_BYTES)
    {
        return None;
    }
    // A conservative preflight also bounds deeply nested parser/formatter work.
    let mut depth = 0usize;
    for byte in source.bytes() {
        match byte {
            b'(' | b'[' | b'{' => {
                depth += 1;
                if depth > 64 {
                    return None;
                }
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let formatted = match language {
        Language::Python => format_module_source(
            source,
            PyFormatOptions::default().with_quote_style(QuoteStyle::Preserve),
        )
        .ok()?
        .into_code(),
        Language::JavaScript => {
            static CONFIG: OnceLock<Configuration> = OnceLock::new();
            format_text(FormatTextOptions {
                path: Path::new("stdin.js"),
                extension: Some("js"),
                text: source.to_owned(),
                config: CONFIG.get_or_init(|| ConfigurationBuilder::new().line_width(88).build()),
                external_formatter: None,
            })
            .ok()?
            .unwrap_or_else(|| source.to_owned())
        }
    };
    (formatted.len() <= MAX_SOURCE_BYTES).then_some(formatted)
}
