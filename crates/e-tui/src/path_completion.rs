//! Owned path-completion values; directory I/O belongs to executable adapters.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCompletionRequest {
    pub cwd: String,
    pub buffer: String,
    pub cursor: usize,
    pub query: String,
    pub token: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCandidate {
    pub path: String,
    pub label: String,
}

/// Pure policy for matching one directory entry against a completion fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathNameMatcher {
    needle: String,
}

impl PathNameMatcher {
    pub fn new(fragment: &str) -> Self {
        Self {
            needle: fragment.to_lowercase(),
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        !name.chars().any(|c| c.is_control() || c == '"')
            && name.to_lowercase().contains(&self.needle)
    }
}

/// Apply the shared ordering and size policy to already-built candidates.
pub fn finalize_candidates(mut candidates: Vec<PathCandidate>) -> Vec<PathCandidate> {
    candidates.sort_by(|a, b| {
        b.label
            .ends_with('/')
            .cmp(&a.label.ends_with('/'))
            .then_with(|| a.label.cmp(&b.label))
    });
    candidates.truncate(100);
    candidates
}

impl PathCompletionRequest {
    pub fn new(cwd: &str, buffer: &str, cursor: usize) -> Option<Self> {
        if cwd.is_empty() {
            return None;
        }
        let chars: Vec<char> = buffer.chars().collect();
        if cursor > chars.len() {
            return None;
        }
        let start = (0..cursor)
            .rev()
            .find(|&i| chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace()))?;
        let quoted = chars.get(start + 1) == Some(&'"');
        let query_start = start + 1 + usize::from(quoted);
        if cursor < query_start {
            return None;
        }
        let mut query_end = cursor;
        if quoted && chars.get(cursor.wrapping_sub(1)) == Some(&'"') && cursor > query_start {
            query_end -= 1;
        }
        let query: String = chars[query_start..query_end].iter().collect();
        if query.contains(['"', '\n', '\r']) || (!quoted && query.chars().any(char::is_whitespace))
        {
            return None;
        }
        let end = if quoted {
            if query_end < cursor {
                cursor
            } else {
                chars[cursor..]
                    .iter()
                    .position(|c| *c == '"')
                    .map_or(cursor, |i| cursor + i + 1)
            }
        } else {
            chars[cursor..]
                .iter()
                .position(|c| c.is_whitespace())
                .map_or(chars.len(), |i| cursor + i)
        };
        Some(Self {
            cwd: cwd.into(),
            buffer: buffer.into(),
            cursor,
            query: query.replace('\\', "/"),
            token: start..end,
        })
    }

    pub fn fill(&self, path: &str) -> (String, usize) {
        let value = if path.chars().any(char::is_whitespace) {
            format!("@\"{path}\"")
        } else {
            format!("@{path}")
        };
        let before: String = self.buffer.chars().take(self.token.start).collect();
        let after: String = self.buffer.chars().skip(self.token.end).collect();
        let cursor = self.token.start + value.chars().count();
        (format!("{before}{value}{after}"), cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(path: &str, label: &str) -> PathCandidate {
        PathCandidate {
            path: path.into(),
            label: label.into(),
        }
    }

    #[test]
    fn candidate_policy_matches_case_insensitive_substrings() {
        let matcher = PathNameMatcher::new("eA");
        assert!(matcher.matches("readme"));
        assert!(matcher.matches("README.md"));
        assert!(!matcher.matches("rm"));
        assert!(!matcher.matches("read\"me"));
        assert!(!matcher.matches("read\u{0000}me"));
        assert!(PathNameMatcher::new("").matches("anything"));
    }

    #[test]
    fn candidate_policy_preserves_spelling_orders_directories_and_limits_results() {
        let candidates = finalize_candidates(vec![
            candidate("README.md", "README.md"),
            candidate("z-file", "z-file"),
            candidate("readme/", "readme/"),
            candidate("Bridge/", "Bridge/"),
            candidate("alpha/", "alpha/"),
        ]);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.path.as_str())
                .collect::<Vec<_>>(),
            ["Bridge/", "alpha/", "readme/", "README.md", "z-file"]
        );

        let limited = finalize_candidates(
            (0..101)
                .map(|index| candidate(&format!("file-{index:03}"), &format!("file-{index:03}")))
                .collect(),
        );
        assert_eq!(limited.len(), 100);
        assert_eq!(limited.first().unwrap().path, "file-000");
        assert_eq!(limited.last().unwrap().path, "file-099");
    }

    #[test]
    fn tokens_preserve_unicode_surroundings_and_quote_spaces() {
        let request = PathCompletionRequest::new("root", "看 @fo 后面", 5).unwrap();
        assert_eq!(request.query, "fo");
        assert_eq!(request.fill("foo/a.rs"), ("看 @foo/a.rs 后面".into(), 11));
        let (buffer, cursor) = request.fill("foo bar/");
        let quoted = PathCompletionRequest::new("root", &buffer, cursor).unwrap();
        assert_eq!(quoted.query, "foo bar/");
        assert_eq!(quoted.fill("foo bar/a.rs").0, "看 @\"foo bar/a.rs\" 后面");
        assert!(PathCompletionRequest::new("root", "mail@foo", 8).is_none());
        assert!(PathCompletionRequest::new("root", "@foo done", 9).is_none());
    }
}
