//! Pure quick-copy discovery and state; workspace probes belong to adapters.

use std::collections::HashSet;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::display::DisplayId;

pub const TAGS: &str = "1234567890abcdefghijklmnopqrstuvwxyz";
pub const MAX_CANDIDATES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathConfidence {
    High,
    Medium,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCandidate {
    pub target: String,
    pub relative: Option<String>,
    pub confidence: PathConfidence,
    pub retain_missing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathValidation {
    Exists,
    Missing,
    Rejected,
}

pub use crate::display::TaggedLink;

#[derive(Debug, Clone)]
pub struct LinkValidationRequest {
    pub generation: u64,
    pub cwd: String,
    pub candidates: Vec<LinkCandidate>,
}

#[derive(Debug, Default)]
pub struct LinkCopyState {
    pub owner: Option<DisplayId>,
    source: String,
    cwd: String,
    generation: u64,
    pub links: Vec<TaggedLink>,
    pub armed: bool,
}

impl LinkCopyState {
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.owner = None;
        self.source.clear();
        self.links.clear();
        self.armed = false;
    }

    pub fn select(
        &mut self,
        owner: DisplayId,
        source: &str,
        cwd: &str,
    ) -> Option<LinkValidationRequest> {
        if self.owner.as_ref() == Some(&owner) && self.source == source && self.cwd == cwd {
            return None;
        }
        self.clear();
        self.owner = Some(owner);
        self.source = source.to_owned();
        self.cwd = cwd.to_owned();
        Some(LinkValidationRequest {
            generation: self.generation,
            cwd: cwd.into(),
            candidates: discover(source),
        })
    }

    pub fn complete(
        &mut self,
        request: &LinkValidationRequest,
        validations: &[PathValidation],
    ) -> bool {
        if self.owner.is_none() || self.generation != request.generation || self.cwd != request.cwd
        {
            return false;
        }
        self.links = request
            .candidates
            .iter()
            .enumerate()
            .filter(|(index, candidate)| {
                candidate.relative.is_none()
                    || match validations.get(*index) {
                        Some(PathValidation::Exists) => true,
                        Some(PathValidation::Missing) => candidate.retain_missing,
                        _ => false,
                    }
            })
            .zip(TAGS.chars())
            .map(|((_, candidate), tag)| TaggedLink {
                target: candidate.target.clone(),
                tag,
            })
            .collect();
        true
    }

    pub fn target(&self, tag: char) -> Option<String> {
        self.links
            .iter()
            .find(|link| link.tag == tag)
            .map(|link| link.target.clone())
    }
}

fn relative_path(text: &str) -> Option<String> {
    let normalized = text.replace('\\', "/");
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part.contains(':') => return None,
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn classify(text: &str, delimited: bool) -> Option<LinkCandidate> {
    if text.is_empty()
        || text.chars().all(|c| matches!(c, '/' | '\\'))
        || text.chars().any(char::is_control)
    {
        return None;
    }
    let absolute = text.starts_with('/')
        || text.starts_with("\\\\")
        || (text.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && text.as_bytes().get(1) == Some(&b':')
            && matches!(text.as_bytes().get(2), Some(b'/' | b'\\')));
    let uri = text.split_once(':').is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && !rest.chars().any(char::is_whitespace)
            && !rest.starts_with(':')
            && scheme
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if absolute || uri {
        return Some(LinkCandidate {
            target: text.into(),
            relative: None,
            confidence: PathConfidence::High,
            retain_missing: true,
        });
    }
    if text.contains(['=', ';', '{', '}', '"', '\'', '`', '|', '*', '<', '>']) {
        return None;
    }
    let separator = text.contains(['/', '\\']);
    let filename = text.rsplit(['/', '\\']).next().unwrap_or(text);
    let extension = filename.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty() && extension.chars().any(char::is_alphabetic)
    });
    let explicit = text.starts_with("./")
        || text.starts_with(".\\")
        || text.starts_with("../")
        || text.starts_with("..\\");
    let dotfile = text.starts_with('.') && text.len() > 1;
    let directory = text.ends_with(['/', '\\']);
    let conventional = matches!(
        text,
        "README"
            | "Makefile"
            | "Dockerfile"
            | "LICENSE"
            | "COPYING"
            | "NOTICE"
            | "Justfile"
            | "Gemfile"
            | "Procfile"
    ) || (text.len() > 1
        && text.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
    if !(separator || extension || explicit || dotfile || directory || conventional || delimited) {
        return None;
    }
    Some(LinkCandidate {
        target: text.into(),
        relative: Some(relative_path(text)?),
        confidence: if explicit || extension || dotfile || directory {
            PathConfidence::High
        } else {
            PathConfidence::Medium
        },
        retain_missing: explicit || dotfile || directory || (separator && extension),
    })
}

fn trim_token(mut text: &str) -> &str {
    text = text.trim_start_matches(['(', '[', '{']);
    text = text.trim_end_matches([
        '.', ',', ';', ':', '!', '?', '，', '。', '；', '：', '！', '？',
    ]);
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        while text.ends_with(close) && text.matches(close).count() > text.matches(open).count() {
            text = &text[..text.len() - close.len_utf8()];
        }
    }
    text
}

fn scan_text(mut text: &str, add: &mut impl FnMut(&str, bool)) {
    let delimiter = |c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '，' | '。' | '；' | '：')
    };
    while let Some(first) = text.chars().next() {
        if matches!(first, '"' | '\'') {
            let rest = &text[first.len_utf8()..];
            if let Some(end) = rest.find(first) {
                add(&rest[..end], true);
                text = &rest[end + first.len_utf8()..];
                continue;
            }
        }
        if delimiter(first) {
            text = &text[first.len_utf8()..];
            continue;
        }
        let end = text.find(delimiter).unwrap_or(text.len());
        let token = trim_token(&text[..end]);
        let rest = text[end..].trim_start();
        let is_unquoted_command_placeholder = token.starts_with('/')
            && rest.starts_with('<')
            && rest.find('>').is_some_and(|close| {
                rest[close + 1..]
                    .chars()
                    .next()
                    .is_none_or(|next| delimiter(next))
            });
        if !is_unquoted_command_placeholder {
            add(token, false);
        }
        text = &text[end..];
    }
}

pub fn discover(source: &str) -> Vec<LinkCandidate> {
    let words: Vec<_> = source.split_whitespace().collect();
    let mut command_tokens: HashSet<String> = words
        .windows(2)
        .filter_map(|pair| {
            (pair[0].starts_with('/') && pair[1].starts_with('<') && pair[1].ends_with('>'))
                .then(|| trim_token(pair[0]).to_owned())
        })
        .collect();
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let quoted = source
        .match_indices('"')
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for pair in quoted.chunks_exact(2) {
        let target = &source[pair[0] + 1..pair[1]];
        if target.starts_with('/') && target.contains('<') && target.contains('>') {
            if let Some(candidate) = classify(target, true) {
                command_tokens.insert(
                    target
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                );
                seen.insert(candidate.target.clone());
                candidates.push(candidate);
            }
        }
    }
    let mut add = |text: &str, delimited: bool| {
        if candidates.len() >= MAX_CANDIDATES {
            return;
        }
        if let Some(candidate) = classify(text, delimited) {
            if !command_tokens.contains(&candidate.target) && seen.insert(candidate.target.clone())
            {
                candidates.push(candidate);
            }
        }
    };
    let mut destinations = Vec::new();
    for event in Parser::new(source) {
        match event {
            Event::Start(Tag::Link { dest_url, .. })
            | Event::Start(Tag::Image { dest_url, .. }) => destinations.push(dest_url),
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some(destination) = destinations.pop() {
                    add(&destination, true);
                }
            }
            Event::Code(text) => {
                if classify(&text, true).is_some() {
                    add(&text, true);
                } else {
                    for word in text.split_whitespace() {
                        add(trim_token(word), false);
                    }
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                scan_text(&text, &mut add);
            }
            _ => {}
        }
    }
    candidates
}

/// Insert suffixes before wrapping while preserving the original styled spans.
pub fn annotate(line: &mut Line<'static>, links: &[TaggedLink], style: Style) {
    if links.is_empty() {
        return;
    }
    let text: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let mut suffixes = Vec::new();
    for link in links {
        for (start, _) in text.match_indices(&link.target) {
            let end = start + link.target.len();
            let boundary = |c: char| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '(' | ')'
                            | '['
                            | ']'
                            | '<'
                            | '>'
                            | '"'
                            | '\''
                            | '`'
                            | ','
                            | ';'
                            | '，'
                            | '。'
                            | '；'
                            | ':'
                            | '：'
                    )
            };
            if text[..start].chars().next_back().is_none_or(boundary)
                && text[end..].chars().next().is_none_or(|c| {
                    if matches!(c, '.' | ',' | ';' | ':' | '!' | '?') {
                        text[end + c.len_utf8()..]
                            .chars()
                            .next()
                            .is_none_or(boundary)
                    } else {
                        boundary(c)
                    }
                })
            {
                suffixes.push((end, link.tag));
            }
        }
    }
    suffixes.sort_unstable();
    suffixes.dedup_by_key(|entry| entry.0);
    let mut suffixes = suffixes.into_iter().peekable();
    let mut spans = Vec::new();
    let mut offset = 0;
    for span in std::mem::take(&mut line.spans) {
        let end = offset + span.content.len();
        let mut local = 0;
        while let Some(&(position, tag)) = suffixes.peek().filter(|entry| entry.0 <= end) {
            let cut = position - offset;
            if cut > local {
                spans.push(Span::styled(
                    span.content[local..cut].to_owned(),
                    span.style,
                ));
            }
            spans.push(Span::styled(format!("~{tag}"), style));
            local = cut;
            suffixes.next();
        }
        if local < span.content.len() {
            spans.push(Span::styled(span.content[local..].to_owned(), span.style));
        }
        offset = end;
    }
    line.spans = spans;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_links_after_chinese_colon_are_discovered_and_tagged() {
        let examples = [
            ("项目主页", "https://github.com/earendil-works/pi"),
            (
                "文档站",
                "https://doc.rust-lang.org/std/string/struct.String.html",
            ),
            ("邮箱式", "mailto:dev@example.com"),
        ];
        let source = examples
            .iter()
            .map(|(label, target)| format!("- {label}：{target}"))
            .collect::<Vec<_>>()
            .join("\n");
        let candidates = discover(&source);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            examples.map(|(_, target)| target)
        );
        assert!(candidates
            .iter()
            .all(|candidate| candidate.relative.is_none()));
        let links: Vec<_> = candidates
            .into_iter()
            .zip(TAGS.chars())
            .map(|(candidate, tag)| TaggedLink {
                target: candidate.target,
                tag,
            })
            .collect();
        for ((label, target), link) in examples.iter().zip(&links) {
            let mut line = Line::raw(format!("◦ {label}：{target}"));
            annotate(&mut line, &links, Style::default());
            assert_eq!(
                line.to_string(),
                format!("◦ {label}：{target}~{}", link.tag)
            );
        }
    }

    #[test]
    fn quick_links_reject_bare_slash() {
        for source in [
            "/",
            "left / right",
            "(/), /.",
            "`/`",
            "\"/\" '/'",
            "```text\n/\n```",
            "[root](/)",
            "![root](/)",
        ] {
            assert!(discover(source).is_empty(), "source: {source:?}");
        }
        assert_eq!(
            discover("/ /tmp /tmp/ https://example.com/ src/main")
                .iter()
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            ["/tmp", "/tmp/", "https://example.com/", "src/main"]
        );
    }

    #[test]
    fn quick_links_reject_separator_only_targets_in_all_contexts() {
        for target in ["/", "//", "///", r"\", r"\\", r"/\/"] {
            for source in [
                target.to_owned(),
                format!("left ({target}), right"),
                format!("`{target}`"),
                format!("\"{target}\""),
                format!("```text\n{target}\n```"),
                format!("[root]({target})"),
                format!("![root]({target})"),
            ] {
                assert!(discover(&source).is_empty(), "source: {source:?}");
            }
        }
    }

    #[test]
    fn quick_links_reject_cpp_scopes_but_keep_real_targets() {
        let code = "std::array<WorldEmitterArchetype, PARTICLE_SYSTEM_MAX_EMITTERS> slots = {};\n\
                    std::array<uint32_t, PARTICLE_SYSTEM_MAX_EMITTERS> entrypoints = {};\n\
                    const uint32_t entrypoint = drh1::work_graph_entrypoint_index(\n\
                    lookup.program->entrypoints, L\"prepare_by_behavior\", group.behavior_index);\n\
                    // comment";
        for source in [code.to_owned(), format!("```cpp\n{code}\n```")] {
            assert!(
                discover(&source)
                    .iter()
                    .all(|candidate| candidate.relative.is_some()),
                "false URI/absolute path: {:?}",
                discover(&source)
            );
        }
        for target in ["std::array", "drh1::work_graph_entrypoint_index()"] {
            for source in [target.to_owned(), format!("`{target}`")] {
                assert!(discover(&source).is_empty(), "source: {source:?}");
            }
        }
        let targets = [
            "https://example.com",
            "mailto:a@example.com",
            "custom:resource",
            "https://[::1]/",
            "https://example.com/std::array",
            "/tmp/file",
            "//server/share",
            r"\\server\share",
            r"C:\src\main.cpp",
        ];
        for source in [
            targets.map(|target| format!("`{target}`")).join(" "),
            format!("```text\n{}\n```", targets.join(" ")),
        ] {
            assert_eq!(
                discover(&source)
                    .iter()
                    .map(|candidate| candidate.target.as_str())
                    .collect::<Vec<_>>(),
                targets
            );
        }
    }

    #[test]
    fn quick_links_reject_uri_whitespace_but_keep_paths_and_encoded_uris() {
        for text in [
            "feat: refine skill reads, preview and link targets",
            "note:some ordinary prose",
            "note:\u{a0}prose",
            "note:\u{3000}prose",
        ] {
            for source in [
                text.to_owned(),
                format!("`{text}`"),
                format!("\"{text}\""),
                format!("```text\n{text}\n```"),
            ] {
                assert!(
                    discover(&source)
                        .iter()
                        .all(|candidate| candidate.target != text),
                    "false URI: {source:?}"
                );
            }
            assert!(classify(text, true).is_none(), "target: {text:?}");
        }
        let targets = [
            "custom:resource",
            "custom+app.v1:resource%20name",
            "https://example.com/a%20b",
            "mailto:a@example.com",
            r"C:\Program Files\app.exe",
            "/tmp/my file.txt",
            "src/my file.rs",
        ];
        for source in [
            targets.map(|target| format!("`{target}`")).join(" "),
            targets.map(|target| format!("\"{target}\"")).join(" "),
        ] {
            assert_eq!(
                discover(&source)
                    .iter()
                    .map(|candidate| candidate.target.as_str())
                    .collect::<Vec<_>>(),
                targets
            );
        }
        assert!(discover("`feat: refine skill reads, preview and link targets`").is_empty());
    }

    #[test]
    fn quick_links_do_not_join_space_separated_command_syntax() {
        assert_eq!(
            discover("/opsx-apply <other>")
                .iter()
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
        assert_eq!(
            discover(r#""/opsx-apply <other>""#)
                .iter()
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            ["/opsx-apply <other>"]
        );
    }

    #[test]
    fn quick_links_discover_mixed_paths_without_probing_prose() {
        let candidates = discover(
            r"See https://example.com/docs, C:\Users\test\README.md and /tmp/session.jsonl. `src/main` README Makefile .gitignore foo/ src\lib.rs ../escape `a b/file.rs` [guide](https://example.com/docs)",
        );
        let values: Vec<_> = candidates.iter().map(|c| c.target.as_str()).collect();
        assert_eq!(
            values,
            [
                "https://example.com/docs",
                r"C:\Users\test\README.md",
                "/tmp/session.jsonl",
                "src/main",
                "README",
                "Makefile",
                ".gitignore",
                "foo/",
                r"src\lib.rs",
                "a b/file.rs"
            ]
        );
        assert!(discover("ordinary lowercase prose without paths").is_empty());
        assert_eq!(
            discover("[src/first.rs](src/second.rs)")
                .iter()
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            ["src/first.rs", "src/second.rs"]
        );
        assert_eq!(
            discover(r#""C:\Program Files\app.exe" "src/a b.rs""#)
                .iter()
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            [r"C:\Program Files\app.exe", "src/a b.rs"]
        );
        assert_eq!(relative_path("src/../README").as_deref(), Some("README"));
        assert_eq!(relative_path("src/../../outside"), None);
        assert_eq!(
            discover("mailto:a@example.com https://example.com/a(b).")[1].target,
            "https://example.com/a(b)"
        );
    }

    #[test]
    fn quick_links_capacity_validation_and_stale_generation() {
        let mut state = LinkCopyState::default();
        let source = (0..40)
            .map(|i| format!("https://example.com/{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let request = state
            .select(DisplayId::event(1, "answer"), &source, "root")
            .unwrap();
        assert!(state.complete(&request, &[]));
        assert_eq!(state.links.len(), 36);
        assert_eq!(state.links[0].tag, '1');
        assert_eq!(state.links[9].tag, '0');
        assert_eq!(state.links[35].tag, 'z');
        state.clear();
        assert!(!state.complete(&request, &[]));
        let request = state
            .select(
                DisplayId::event(2, "answer"),
                "src/main ./missing.txt ./escape.txt README",
                "root",
            )
            .unwrap();
        assert!(state.complete(
            &request,
            &[
                PathValidation::Exists,
                PathValidation::Missing,
                PathValidation::Rejected,
                PathValidation::Missing
            ]
        ));
        assert_eq!(
            state
                .links
                .iter()
                .map(|link| link.target.as_str())
                .collect::<Vec<_>>(),
            ["src/main", "./missing.txt"]
        );
    }

    #[test]
    fn quick_links_suffix_crosses_styles_without_changing_source_text() {
        let mut line = Line::from(vec![
            Span::raw("see src/"),
            Span::raw("main and src/mainly"),
        ]);
        annotate(
            &mut line,
            &[TaggedLink {
                target: "src/main".into(),
                tag: '1',
            }],
            Style::default(),
        );
        assert_eq!(
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "see src/main~1 and src/mainly"
        );
    }
}
