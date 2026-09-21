//! Pure quick-copy discovery and state; workspace probes belong to adapters.

use std::collections::HashSet;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::display::DisplayId;

pub(crate) const NON_COPYABLE_MODIFIER: Modifier = Modifier::from_bits_retain(1 << 15);
const _: () = assert!(Modifier::all().bits() & NON_COPYABLE_MODIFIER.bits() == 0);

pub const TAGS: &str = "1234567890abcdefghijklmnopqrstuvwxyz";
pub const MAX_CANDIDATES: usize = 256;
const MAX_ALTERNATIVES_PER_GROUP: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathConfidence {
    High,
    Medium,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTargetKind {
    Uri,
    AbsolutePath,
    WorkspaceRelative { normalized: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCandidate {
    pub target: String,
    pub kind: LinkTargetKind,
    pub confidence: PathConfidence,
    pub retain_missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCandidateGroup {
    pub alternatives: Vec<LinkCandidate>,
    pub require_existing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathValidation {
    NotRequired,
    Exists,
    Missing,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateGroupValidation {
    pub alternatives: Vec<PathValidation>,
}

pub use crate::display::TaggedLink;

#[derive(Debug, Clone)]
pub struct LinkValidationRequest {
    pub generation: u64,
    pub cwd: String,
    pub groups: Vec<LinkCandidateGroup>,
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
            groups: discover(source),
        })
    }

    pub fn complete(
        &mut self,
        request: &LinkValidationRequest,
        validations: &[CandidateGroupValidation],
    ) -> bool {
        if self.owner.is_none() || self.generation != request.generation || self.cwd != request.cwd
        {
            return false;
        }
        let mut seen = HashSet::new();
        self.links = request
            .groups
            .iter()
            .zip(validations)
            .filter_map(|(group, validation)| resolve_group(group, validation))
            .filter(|candidate| seen.insert(candidate.target.clone()))
            .zip(TAGS.chars())
            .map(|(candidate, tag)| TaggedLink {
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

fn resolve_group<'a>(
    group: &'a LinkCandidateGroup,
    validation: &CandidateGroupValidation,
) -> Option<&'a LinkCandidate> {
    if group.alternatives.len() != validation.alternatives.len() {
        return None;
    }
    for (candidate, status) in group.alternatives.iter().zip(&validation.alternatives) {
        let accepted = matches!(
            (&candidate.kind, status),
            (LinkTargetKind::Uri, PathValidation::NotRequired)
                | (
                    LinkTargetKind::AbsolutePath | LinkTargetKind::WorkspaceRelative { .. },
                    PathValidation::Exists
                )
        );
        if accepted {
            return Some(candidate);
        }
    }
    if group.require_existing || group.alternatives.len() != 1 {
        return None;
    }
    group.alternatives.first().filter(|candidate| {
        matches!(candidate.kind, LinkTargetKind::WorkspaceRelative { .. })
            && candidate.retain_missing
            && validation.alternatives.first() == Some(&PathValidation::Missing)
    })
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
        || text.chars().all(char::is_whitespace)
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
    if absolute {
        return Some(LinkCandidate {
            target: text.into(),
            kind: LinkTargetKind::AbsolutePath,
            confidence: PathConfidence::High,
            retain_missing: false,
        });
    }
    if uri {
        return Some(LinkCandidate {
            target: text.into(),
            kind: LinkTargetKind::Uri,
            confidence: PathConfidence::High,
            retain_missing: false,
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
        kind: LinkTargetKind::WorkspaceRelative {
            normalized: relative_path(text)?,
        },
        confidence: if explicit || extension || dotfile || directory {
            PathConfidence::High
        } else {
            PathConfidence::Medium
        },
        retain_missing: explicit || dotfile || directory || (separator && extension),
    })
}

fn is_soft_boundary(c: char) -> bool {
    matches!(c, '（' | '【' | '〔' | '《' | '〈' | '［' | '｛')
}

fn candidate_group(text: &str, delimited: bool) -> Option<LinkCandidateGroup> {
    let complete = classify(text, delimited);
    if complete
        .as_ref()
        .is_some_and(|candidate| matches!(candidate.kind, LinkTargetKind::Uri))
    {
        return complete.map(|candidate| LinkCandidateGroup {
            alternatives: vec![candidate],
            require_existing: false,
        });
    }

    let mut alternatives = complete.into_iter().collect::<Vec<_>>();
    let mut derived = false;
    let boundaries = text
        .char_indices()
        .filter_map(|(index, c)| is_soft_boundary(c).then_some(index))
        .collect::<Vec<_>>();
    for boundary in boundaries.into_iter().rev() {
        if alternatives.len() >= MAX_ALTERNATIVES_PER_GROUP {
            break;
        }
        let prefix = trim_token(&text[..boundary]);
        let Some(candidate) = classify(prefix, delimited) else {
            continue;
        };
        if matches!(candidate.kind, LinkTargetKind::Uri)
            || alternatives
                .iter()
                .any(|existing| existing.target == candidate.target)
        {
            continue;
        }
        derived = true;
        alternatives.push(candidate);
    }
    alternatives.sort_by(|left, right| right.target.len().cmp(&left.target.len()));
    alternatives.truncate(MAX_ALTERNATIVES_PER_GROUP);
    (!alternatives.is_empty()).then_some(LinkCandidateGroup {
        alternatives,
        require_existing: derived,
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

fn is_hard_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '，' | '。' | '；' | '：')
}

fn is_target_boundary(c: char) -> bool {
    is_hard_boundary(c)
        || is_soft_boundary(c)
        || matches!(
            c,
            '(' | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | ','
                | ';'
                | ':'
                | '！'
                | '？'
                | '）'
                | '】'
                | '〕'
                | '》'
                | '〉'
                | '］'
                | '｝'
        )
}

fn scan_text(mut text: &str, base_position: usize, add: &mut impl FnMut(&str, bool, usize)) {
    let delimiter = is_hard_boundary;
    let mut offset = 0;
    while let Some(first) = text.chars().next() {
        if matches!(first, '"' | '\'') {
            let rest = &text[first.len_utf8()..];
            if let Some(end) = rest.find(first) {
                add(
                    &rest[..end],
                    true,
                    base_position + offset + first.len_utf8(),
                );
                let consumed = first.len_utf8() + end + first.len_utf8();
                offset += consumed;
                text = &text[consumed..];
                continue;
            }
        }
        if delimiter(first) {
            offset += first.len_utf8();
            text = &text[first.len_utf8()..];
            continue;
        }
        let end = text.find(delimiter).unwrap_or(text.len());
        let raw = &text[..end];
        let token = trim_token(raw);
        let token_offset = raw.find(token).unwrap_or_default();
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
            add(token, false, base_position + offset + token_offset);
        }
        offset += end;
        text = &text[end..];
    }
}

pub fn discover(source: &str) -> Vec<LinkCandidateGroup> {
    let words: Vec<_> = source.split_whitespace().collect();
    let mut command_tokens: HashSet<String> = words
        .windows(2)
        .filter_map(|pair| {
            (pair[0].starts_with('/') && pair[1].starts_with('<') && pair[1].ends_with('>'))
                .then(|| trim_token(pair[0]).to_owned())
        })
        .collect();
    let quoted = source
        .match_indices('"')
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let quoted_targets = quoted
        .chunks_exact(2)
        .map(|pair| {
            (
                pair[0]..pair[1] + 1,
                source[pair[0] + 1..pair[1]].to_owned(),
            )
        })
        .collect::<Vec<_>>();
    for (_, target) in quoted_targets.iter().filter(|(_, target)| {
        target.starts_with('/') && target.contains('<') && target.contains('>')
    }) {
        command_tokens.insert(
            target
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
    }

    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    let mut next_order = 0;
    let mut add_group = |group: LinkCandidateGroup, position: usize| {
        if group
            .alternatives
            .iter()
            .any(|candidate| command_tokens.contains(&candidate.target))
        {
            return;
        }
        let signature = (
            group
                .alternatives
                .iter()
                .map(|candidate| candidate.target.clone())
                .collect::<Vec<_>>(),
            group.require_existing,
        );
        if seen.insert(signature) {
            groups.push((position, next_order, group));
            next_order += 1;
        }
    };
    for (range, target) in &quoted_targets {
        if let Some(group) = candidate_group(target, true) {
            add_group(group, range.start + 1);
        }
    }
    let mut add = |text: &str, delimited: bool, position: usize| {
        if quoted_targets.iter().any(|(range, quoted)| {
            range.contains(&position) && quoted != text && quoted.contains(text)
        }) {
            return;
        }
        if let Some(group) = candidate_group(text, delimited) {
            add_group(group, position);
        }
    };
    let mut destinations = Vec::new();
    for (event, range) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. })
            | Event::Start(Tag::Image { dest_url, .. }) => {
                let position = source[range.clone()]
                    .find(dest_url.as_ref())
                    .map(|offset| range.start + offset)
                    .unwrap_or(range.start);
                destinations.push((dest_url, position));
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some((destination, position)) = destinations.pop() {
                    add(&destination, true, position);
                }
            }
            Event::Code(text) => {
                let position = source[range.clone()]
                    .find(text.as_ref())
                    .map(|offset| range.start + offset)
                    .unwrap_or(range.start);
                if candidate_group(&text, true).is_some() {
                    add(&text, true, position);
                } else {
                    scan_text(&text, position, &mut add);
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                scan_text(&text, range.start, &mut add);
            }
            _ => {}
        }
    }
    groups.sort_by_key(|(position, order, _)| (*position, *order));
    let groups = groups
        .into_iter()
        .map(|(_, _, group)| group)
        .collect::<Vec<_>>();
    let mut filesystem_hypotheses = 0;
    let mut bounded = Vec::new();
    for mut group in groups {
        if bounded.len() >= MAX_CANDIDATES {
            break;
        }
        let filesystem_count = group
            .alternatives
            .iter()
            .filter(|candidate| !matches!(candidate.kind, LinkTargetKind::Uri))
            .count();
        let remaining = MAX_CANDIDATES.saturating_sub(filesystem_hypotheses);
        if filesystem_count > remaining {
            group.alternatives.truncate(remaining);
        }
        if group.alternatives.is_empty() {
            continue;
        }
        filesystem_hypotheses += group
            .alternatives
            .iter()
            .filter(|candidate| !matches!(candidate.kind, LinkTargetKind::Uri))
            .count();
        bounded.push(group);
    }
    bounded
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
    let mut matches = Vec::new();
    for (order, link) in links.iter().enumerate() {
        for (start, _) in text.match_indices(&link.target) {
            let end = start + link.target.len();
            if text[..start]
                .chars()
                .next_back()
                .is_none_or(is_target_boundary)
                && text[end..].chars().next().is_none_or(|c| {
                    if matches!(c, '.' | ',' | ';' | ':' | '!' | '?') {
                        text[end + c.len_utf8()..]
                            .chars()
                            .next()
                            .is_none_or(is_target_boundary)
                    } else {
                        is_target_boundary(c)
                    }
                })
            {
                matches.push((start, end, order, link.tag));
            }
        }
    }
    matches.sort_unstable_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| (right.1 - right.0).cmp(&(left.1 - left.0)))
            .then_with(|| left.2.cmp(&right.2))
    });
    let mut selected: Vec<(usize, usize, char)> = Vec::new();
    for (start, end, _, tag) in matches {
        if selected.iter().all(|(selected_start, selected_end, _)| {
            end <= *selected_start || start >= *selected_end
        }) {
            selected.push((start, end, tag));
        }
    }
    let mut suffixes = selected
        .into_iter()
        .map(|(_, end, tag)| (end, tag))
        .collect::<Vec<_>>();
    suffixes.sort_unstable();
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
            spans.push(Span::styled(
                format!("~{tag}"),
                style.add_modifier(NON_COPYABLE_MODIFIER),
            ));
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

    fn candidates(groups: &[LinkCandidateGroup]) -> impl Iterator<Item = &LinkCandidate> {
        groups.iter().flat_map(|group| group.alternatives.iter())
    }

    fn targets(source: &str) -> Vec<String> {
        candidates(&discover(source))
            .map(|candidate| candidate.target.clone())
            .collect()
    }

    fn validation(alternatives: &[PathValidation]) -> CandidateGroupValidation {
        CandidateGroupValidation {
            alternatives: alternatives.to_vec(),
        }
    }

    fn uri_validations(request: &LinkValidationRequest) -> Vec<CandidateGroupValidation> {
        request
            .groups
            .iter()
            .map(|group| CandidateGroupValidation {
                alternatives: vec![PathValidation::NotRequired; group.alternatives.len()],
            })
            .collect()
    }

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
        let groups = discover(&source);
        assert_eq!(
            candidates(&groups)
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            examples.map(|(_, target)| target)
        );
        assert!(candidates(&groups).all(|candidate| matches!(candidate.kind, LinkTargetKind::Uri)));
        let links: Vec<_> = candidates(&groups)
            .zip(TAGS.chars())
            .map(|(candidate, tag)| TaggedLink {
                target: candidate.target.clone(),
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
            targets("/ /tmp /tmp/ https://example.com/ src/main"),
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
            let groups = discover(&source);
            assert!(
                candidates(&groups).all(|candidate| {
                    matches!(candidate.kind, LinkTargetKind::WorkspaceRelative { .. })
                }),
                "false URI/absolute path: {groups:?}"
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
            assert_eq!(super::tests::targets(&source), targets);
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
                    targets(&source).iter().all(|candidate| candidate != text),
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
            assert_eq!(super::tests::targets(&source), targets);
        }
        assert!(discover("`feat: refine skill reads, preview and link targets`").is_empty());
    }

    #[test]
    fn quick_links_do_not_join_space_separated_command_syntax() {
        assert_eq!(targets("/opsx-apply <other>"), Vec::<&str>::new());
        assert_eq!(targets(r#""/opsx-apply <other>""#), ["/opsx-apply <other>"]);
    }

    #[test]
    fn quick_links_discover_mixed_paths_without_probing_prose() {
        let groups = discover(
            r"See https://example.com/docs, C:\Users\test\README.md and /tmp/session.jsonl. `src/main` README Makefile .gitignore foo/ src\lib.rs ../escape `a b/file.rs` [guide](https://example.com/docs)",
        );
        let values: Vec<_> = candidates(&groups)
            .map(|candidate| candidate.target.as_str())
            .collect();
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
            targets("[src/first.rs](src/second.rs)"),
            ["src/first.rs", "src/second.rs"]
        );
        assert_eq!(
            targets(r#""C:\Program Files\app.exe" "src/a b.rs""#),
            [r"C:\Program Files\app.exe", "src/a b.rs"]
        );
        assert_eq!(relative_path("src/../README").as_deref(), Some("README"));
        assert_eq!(relative_path("src/../../outside"), None);
        assert_eq!(
            discover("mailto:a@example.com https://example.com/a(b).")[1].alternatives[0].target,
            "https://example.com/a(b)"
        );
    }

    #[test]
    fn quick_links_group_chinese_wrapper_hypotheses_and_require_existence() {
        let source = "final-report/2_virtual_geometry_demo.html（+340/−7 行）";
        let groups = discover(source);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].require_existing);
        assert_eq!(
            groups[0]
                .alternatives
                .iter()
                .map(|candidate| candidate.target.as_str())
                .collect::<Vec<_>>(),
            [
                "final-report/2_virtual_geometry_demo.html（+340/−7",
                "final-report/2_virtual_geometry_demo.html",
            ]
        );

        let resolve = |statuses: &[PathValidation]| {
            let mut state = LinkCopyState::default();
            let request = state
                .select(DisplayId::event(1, "answer"), source, "root")
                .unwrap();
            assert!(state.complete(&request, &[validation(statuses)]));
            state.links.first().map(|link| link.target.clone())
        };
        assert_eq!(
            resolve(&[PathValidation::Missing, PathValidation::Exists]).as_deref(),
            Some("final-report/2_virtual_geometry_demo.html")
        );
        assert_eq!(
            resolve(&[PathValidation::Exists, PathValidation::Exists]).as_deref(),
            Some("final-report/2_virtual_geometry_demo.html（+340/−7")
        );
        assert_eq!(
            resolve(&[PathValidation::Exists, PathValidation::Missing]).as_deref(),
            Some("final-report/2_virtual_geometry_demo.html（+340/−7")
        );
        assert_eq!(
            resolve(&[PathValidation::Missing, PathValidation::Missing]),
            None
        );
    }

    #[test]
    fn quick_links_soft_boundaries_annotate_and_longest_overlap_wins() {
        let wrappers = ['（', '【', '〔', '《', '〈', '［', '｛'];
        let mut line = Line::raw(
            wrappers
                .iter()
                .map(|wrapper| format!("src/main.rs{wrapper}note"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        annotate(
            &mut line,
            &[TaggedLink {
                target: "src/main.rs".into(),
                tag: '1',
            }],
            Style::default(),
        );
        for wrapper in wrappers {
            assert!(line
                .to_string()
                .contains(&format!("src/main.rs~1{wrapper}")));
        }

        let mut line = Line::raw("path（note） path");
        annotate(
            &mut line,
            &[
                TaggedLink {
                    target: "path".into(),
                    tag: '1',
                },
                TaggedLink {
                    target: "path（note）".into(),
                    tag: '2',
                },
            ],
            Style::default(),
        );
        assert_eq!(line.to_string(), "path（note）~2 path~1");
    }

    #[test]
    fn quick_links_bound_total_filesystem_hypotheses() {
        let source = (0..300)
            .map(|index| format!("path/{index}.rs（note"))
            .collect::<Vec<_>>()
            .join(" ");
        let groups = discover(&source);
        assert!(groups.len() <= MAX_CANDIDATES);
        assert!(groups
            .iter()
            .all(|group| group.alternatives.len() <= MAX_ALTERNATIVES_PER_GROUP));
        assert!(
            groups
                .iter()
                .flat_map(|group| &group.alternatives)
                .filter(|candidate| !matches!(candidate.kind, LinkTargetKind::Uri))
                .count()
                <= MAX_CANDIDATES
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
        let validations = uri_validations(&request);
        assert!(state.complete(&request, &validations));
        assert_eq!(state.links.len(), 36);
        assert_eq!(state.links[0].tag, '1');
        assert_eq!(state.links[9].tag, '0');
        assert_eq!(state.links[35].tag, 'z');
        state.clear();
        assert!(!state.complete(&request, &validations));
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
                validation(&[PathValidation::Exists]),
                validation(&[PathValidation::Missing]),
                validation(&[PathValidation::Rejected]),
                validation(&[PathValidation::Missing]),
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
