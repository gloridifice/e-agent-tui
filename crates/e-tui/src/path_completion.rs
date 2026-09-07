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
