//! Kernel-neutral tool presentation facts.

/// Workspace-relative display form of a file path: paths inside the workspace
/// lose the workspace prefix, paths outside keep their absolute form. Both
/// sides are normalized to forward slashes and compared case-insensitively,
/// so Windows and POSIX drives both relativize. Pure string work — the client
/// never reads the file.
pub fn workspace_relative_path(path: &str, workspace: Option<&str>) -> String {
    let normalize = |value: &str| value.trim_end_matches(['/', '\\']).replace('\\', "/");
    let normalized_path = normalize(path);
    let Some(workspace) = workspace else {
        return normalized_path;
    };
    let normalized_workspace = normalize(workspace);
    let path_parts = normalized_path.split('/').collect::<Vec<_>>();
    let workspace_parts = normalized_workspace.split('/').collect::<Vec<_>>();
    let inside = path_parts.len() >= workspace_parts.len()
        && path_parts
            .iter()
            .zip(&workspace_parts)
            .all(|(left, right)| left.eq_ignore_ascii_case(right));
    if !inside {
        return normalized_path;
    }
    let relative = &path_parts[workspace_parts.len()..];
    if relative.is_empty() {
        ".".into()
    } else {
        relative.join("/")
    }
}

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
    Hunks(Vec<crate::preview::MutationHunk>),
    Custom {
        namespace: String,
        kind: String,
        value: String,
    },
}

impl ToolReference {
    /// Path-bearing references rewritten to their workspace-relative display
    /// form (`workspace_relative_path`); every other reference is returned
    /// unchanged.
    pub fn relativized(&self, workspace: Option<&str>) -> ToolReference {
        match self {
            Self::Path { path } => Self::Path {
                path: workspace_relative_path(path, workspace),
            },
            Self::Lines { path, start, lines } => Self::Lines {
                path: workspace_relative_path(path, workspace),
                start: *start,
                lines: lines.clone(),
            },
            Self::Hunks(hunks) => Self::Hunks(
                hunks
                    .iter()
                    .map(|hunk| crate::preview::MutationHunk {
                        path: hunk
                            .path
                            .as_deref()
                            .map(|path| workspace_relative_path(path, workspace)),
                        ..hunk.clone()
                    })
                    .collect(),
            ),
            _ => self.clone(),
        }
    }

    pub fn preview_reference(
        &self,
        key_prefix: &str,
        revision: crate::preview::PreviewRevision,
    ) -> Option<crate::preview::PreviewRef> {
        use crate::preview::{PreviewContent, PreviewKey, PreviewRef};
        match self {
            // File references preview the path itself (already relativized by
            // the caller against the workspace) rather than the file contents:
            // the client never reads the file for a Preview.
            Self::Path { path } => Some(PreviewRef::Inline {
                key: PreviewKey(key_prefix.into()),
                revision,
                content: PreviewContent::Path(path.clone()),
            }),
            Self::Lines { path, lines, .. } if lines.is_empty() => Some(PreviewRef::Inline {
                key: PreviewKey(key_prefix.into()),
                revision,
                content: PreviewContent::Path(path.clone()),
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
            Self::Hunks(hunks) => PreviewContent::Hunks(hunks.clone()),
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
    /// Adapter-provided structured preview seed (common-format tools).
    /// Mutation tools carry their preview through `reference` instead.
    pub preview: Option<crate::preview::ToolPreview>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::{PreviewContent, PreviewRef, PreviewRevision};

    #[test]
    fn file_references_preview_the_path_inline_without_deferring() {
        let revision = PreviewRevision(1);
        for reference in [
            ToolReference::Path {
                path: "src/main.rs".into(),
            },
            ToolReference::Lines {
                path: "src/main.rs".into(),
                start: 3,
                lines: Vec::new(),
            },
        ] {
            let preview = reference.preview_reference("tool:1", revision).unwrap();
            assert!(
                matches!(
                    &preview,
                    PreviewRef::Inline {
                        content: PreviewContent::Path(path),
                        ..
                    } if path == "src/main.rs"
                ),
                "file references must preview the path, never defer content: {preview:?}"
            );
        }
    }

    #[test]
    fn relativized_rewrites_inside_paths_and_keeps_outside_absolute() {
        let workspace = Some(r"G:\workspace");
        let inside = ToolReference::Lines {
            path: r"G:\workspace\src\main.rs".into(),
            start: 1,
            lines: Vec::new(),
        };
        assert!(matches!(
            inside.relativized(workspace),
            ToolReference::Lines { path, .. } if path == "src/main.rs"
        ));
        let already_relative = ToolReference::Path {
            path: "src/main.rs".into(),
        };
        assert!(matches!(
            already_relative.relativized(workspace),
            ToolReference::Path { path } if path == "src/main.rs"
        ));
        let outside = ToolReference::Path {
            path: r"C:\other\lib.rs".into(),
        };
        assert!(matches!(
            outside.relativized(workspace),
            ToolReference::Path { path } if path == "C:/other/lib.rs"
        ));
    }
}
