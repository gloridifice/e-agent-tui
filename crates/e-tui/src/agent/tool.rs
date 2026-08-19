//! Kernel-neutral tool presentation facts.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolCapability {
    Read,
    View,
    Edit,
    Insert,
    Replace,
    Search,
    Command,
    Create,
    Generic,
    Custom { namespace: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Waiting,
    Running,
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolReference {
    Path {
        path: String,
    },
    Link {
        label: Option<String>,
        url: String,
    },
    Text {
        text: String,
    },
    Diff {
        path: Option<String>,
        diff: String,
    },
    Lines {
        path: String,
        start: usize,
        lines: Vec<String>,
    },
    SearchResult {
        query: String,
        matches: Vec<String>,
    },
    Command {
        command: String,
    },
    Markdown {
        source: String,
    },
    PlainText {
        text: String,
    },
    Custom {
        namespace: String,
        kind: String,
        value: String,
    },
}

impl ToolReference {
    pub fn preview_reference(
        &self,
        key_prefix: &str,
        revision: crate::preview::PreviewRevision,
    ) -> Option<crate::preview::PreviewRef> {
        use crate::preview::{PreviewKey, PreviewRef};
        match self {
            Self::Path { path } => Some(PreviewRef::Deferred {
                key: PreviewKey(format!("file:{path}")),
                revision,
            }),
            Self::Lines { path, start, lines } if lines.is_empty() => Some(PreviewRef::Deferred {
                key: PreviewKey(format!("lines:{path}:{start}")),
                revision,
            }),
            _ => self.preview_content().map(|content| PreviewRef::Inline {
                key: PreviewKey(key_prefix.into()),
                revision,
                content,
            }),
        }
    }

    pub fn preview_content(&self) -> Option<crate::preview::PreviewContent> {
        use crate::preview::PreviewContent;
        Some(match self {
            Self::Path { path } => PreviewContent::Path(path.clone()),
            Self::Link { label, url } => PreviewContent::Link {
                label: label.clone(),
                url: url.clone(),
            },
            Self::Text { text } | Self::PlainText { text } => {
                PreviewContent::PlainText(text.clone())
            }
            Self::Diff { diff, .. } => PreviewContent::Diff(diff.clone()),
            Self::Lines { path, start, lines } => PreviewContent::Lines {
                path: path.clone(),
                start: *start,
                lines: lines.clone(),
            },
            Self::SearchResult { query, matches } => PreviewContent::SearchResult {
                query: query.clone(),
                matches: matches.clone(),
            },
            Self::Command { command } => PreviewContent::Command(command.clone()),
            Self::Markdown { source } => PreviewContent::Markdown(source.clone()),
            Self::Custom { .. } => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolItem {
    pub id: String,
    pub label: String,
    pub reference: ToolReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivity {
    pub id: String,
    pub capability: ToolCapability,
    pub label: String,
    pub summary: String,
    pub state: ActivityState,
    pub reference: Option<ToolReference>,
    pub items: Vec<ToolItem>,
}
