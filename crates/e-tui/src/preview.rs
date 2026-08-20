//! Shared normal/Reading Preview target, cache, and race reconciliation.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewPolicy {
    FollowLatestBlock,
    FollowReadingCursor,
}

impl Default for PreviewPolicy {
    fn default() -> Self {
        Self::FollowLatestBlock
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PreviewRequestId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PreviewKey(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PreviewRevision(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequest {
    pub request_id: PreviewRequestId,
    pub key: PreviewKey,
    pub revision: PreviewRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewContent {
    Link {
        label: Option<String>,
        url: String,
    },
    Diff(String),
    Lines {
        path: String,
        start: usize,
        lines: Vec<String>,
    },
    SearchResult {
        query: String,
        matches: Vec<String>,
    },
    Command(String),
    Path(String),
    Markdown(String),
    /// Live model-reasoning text; rendered with the muted (Bark) tone so the
    /// Thinking phase reads as secondary content in the Preview pane.
    Reasoning(String),
    PlainText(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewRef {
    Inline {
        key: PreviewKey,
        revision: PreviewRevision,
        content: PreviewContent,
    },
    Deferred {
        key: PreviewKey,
        revision: PreviewRevision,
    },
}

impl PreviewRef {
    pub fn key(&self) -> &PreviewKey {
        match self {
            Self::Inline { key, .. } | Self::Deferred { key, .. } => key,
        }
    }

    pub fn revision(&self) -> PreviewRevision {
        match self {
            Self::Inline { revision, .. } | Self::Deferred { revision, .. } => *revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewTarget {
    /// Width-independent semantic owner identity.
    pub id: String,
    pub reference: PreviewRef,
}

impl PreviewTarget {
    pub fn complete_source(
        id: impl Into<String>,
        source: impl Into<String>,
        revision: u64,
    ) -> Self {
        let id = id.into();
        Self {
            reference: PreviewRef::Inline {
                key: PreviewKey(format!("source:{id}")),
                revision: PreviewRevision(revision),
                content: PreviewContent::Markdown(source.into()),
            },
            id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewState {
    Empty,
    Loading {
        request_id: PreviewRequestId,
        key: PreviewKey,
        revision: PreviewRevision,
    },
    Ready(PreviewContent),
    Error(String),
}

impl Default for PreviewState {
    fn default() -> Self {
        Self::Empty
    }
}

#[derive(Debug, Default)]
pub struct PreviewCache {
    values: HashMap<(PreviewKey, PreviewRevision), Result<PreviewContent, String>>,
}

impl PreviewCache {
    pub fn get(
        &self,
        key: &PreviewKey,
        revision: PreviewRevision,
    ) -> Option<&Result<PreviewContent, String>> {
        self.values.get(&(key.clone(), revision))
    }

    pub fn insert(
        &mut self,
        key: PreviewKey,
        revision: PreviewRevision,
        result: Result<PreviewContent, String>,
    ) {
        self.values.insert((key, revision), result);
    }

    pub fn invalidate(&mut self, key: &PreviewKey) {
        self.values.retain(|(candidate, _), _| candidate != key);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreviewWorkStats {
    pub rebuilds: u64,
    pub patches: u64,
    pub materialized_rows: u64,
}

#[derive(Debug, Default)]
pub struct PreviewPaneState {
    pub policy: PreviewPolicy,
    pub target: Option<PreviewTarget>,
    pub state: PreviewState,
    pub scroll: usize,
    pub fullscreen: bool,
    pub cache: PreviewCache,
    next_request_id: u64,
    work: PreviewWorkStats,
}

impl PreviewPaneState {
    /// Reconcile one semantic target. Identity changes reset Preview scroll;
    /// revision-only refreshes preserve it. Returns deferred work when cache
    /// reuse or inline presentation cannot satisfy the target.
    pub fn select(&mut self, target: Option<PreviewTarget>) -> Option<PreviewRequest> {
        let identity_changed = self.target.as_ref().map(|target| target.id.as_str())
            != target.as_ref().map(|target| target.id.as_str());
        if identity_changed {
            self.scroll = 0;
            self.work.rebuilds = self.work.rebuilds.saturating_add(1);
        } else if self.target != target {
            self.work.patches = self.work.patches.saturating_add(1);
        }
        self.target = target;
        let Some(target) = self.target.as_ref() else {
            self.state = PreviewState::Empty;
            return None;
        };
        let key = target.reference.key().clone();
        let revision = target.reference.revision();
        if let Some(cached) = self.cache.get(&key, revision) {
            self.state = match cached {
                Ok(content) => PreviewState::Ready(content.clone()),
                Err(error) => PreviewState::Error(error.clone()),
            };
            return None;
        }
        if let PreviewRef::Inline { content, .. } = &target.reference {
            self.cache.insert(key, revision, Ok(content.clone()));
            self.state = PreviewState::Ready(content.clone());
            return None;
        }

        let request_id = PreviewRequestId(self.next_request_id);
        self.next_request_id = self.next_request_id.wrapping_add(1);
        self.state = PreviewState::Loading {
            request_id,
            key: key.clone(),
            revision,
        };
        Some(PreviewRequest {
            request_id,
            key,
            revision,
        })
    }

    /// Cache every bounded completion, but update visible state only when all
    /// race tokens still match the current loading target.
    pub fn complete(
        &mut self,
        request_id: PreviewRequestId,
        key: PreviewKey,
        revision: PreviewRevision,
        result: Result<PreviewContent, String>,
    ) -> bool {
        self.cache.insert(key.clone(), revision, result.clone());
        let visible = matches!(
            &self.state,
            PreviewState::Loading {
                request_id: current_request,
                key: current_key,
                revision: current_revision,
            } if *current_request == request_id
                && *current_key == key
                && *current_revision == revision
        );
        if visible {
            self.work.patches = self.work.patches.saturating_add(1);
            self.state = match result {
                Ok(content) => PreviewState::Ready(content),
                Err(error) => PreviewState::Error(error),
            };
        }
        visible
    }

    pub fn record_materialized_rows(&mut self, rows: usize) {
        self.work.materialized_rows = self.work.materialized_rows.saturating_add(rows as u64);
    }

    pub fn take_work_stats(&mut self) -> PreviewWorkStats {
        std::mem::take(&mut self.work)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deferred(id: &str, key: &str, revision: u64) -> PreviewTarget {
        PreviewTarget {
            id: id.into(),
            reference: PreviewRef::Deferred {
                key: PreviewKey(key.into()),
                revision: PreviewRevision(revision),
            },
        }
    }

    #[test]
    fn target_change_resets_scroll_and_late_completion_only_populates_cache() {
        let mut pane = PreviewPaneState::default();
        pane.scroll = 9;
        let a = pane.select(Some(deferred("a", "a", 1))).unwrap();
        assert_eq!(pane.scroll, 0);
        pane.scroll = 4;
        let b = pane.select(Some(deferred("b", "b", 1))).unwrap();
        assert_eq!(pane.scroll, 0);
        assert!(!pane.complete(
            a.request_id,
            a.key.clone(),
            a.revision,
            Ok(PreviewContent::PlainText("A".into())),
        ));
        assert!(matches!(pane.state, PreviewState::Loading { .. }));
        assert!(pane.complete(
            b.request_id,
            b.key,
            b.revision,
            Ok(PreviewContent::PlainText("B".into())),
        ));
        assert_eq!(
            pane.state,
            PreviewState::Ready(PreviewContent::PlainText("B".into()))
        );
    }

    #[test]
    fn same_target_revision_refresh_preserves_scroll() {
        let mut pane = PreviewPaneState::default();
        pane.select(Some(deferred("a", "a", 1)));
        pane.scroll = 6;
        pane.select(Some(deferred("a", "a", 2)));
        assert_eq!(pane.scroll, 6);
    }

    #[test]
    fn revision_and_error_completions_are_race_safe_across_policies() {
        let mut pane = PreviewPaneState::default();
        let old = pane.select(Some(deferred("a", "a", 1))).unwrap();
        let current = pane.select(Some(deferred("a", "a", 2))).unwrap();
        assert!(!pane.complete(
            old.request_id,
            old.key,
            old.revision,
            Ok(PreviewContent::PlainText("old".into())),
        ));
        pane.policy = PreviewPolicy::FollowReadingCursor;
        assert!(pane.complete(
            current.request_id,
            current.key,
            current.revision,
            Err("bounded error".into()),
        ));
        assert_eq!(pane.state, PreviewState::Error("bounded error".into()));
        pane.policy = PreviewPolicy::FollowLatestBlock;
        assert!(pane.select(Some(deferred("a", "a", 1))).is_none());
        assert_eq!(
            pane.state,
            PreviewState::Ready(PreviewContent::PlainText("old".into()))
        );
    }

    #[test]
    fn target_changes_do_not_invalidate_transcript_cache() {
        let mut app = crate::app::TuiApp::default();
        app.render.transcript_cache.valid = true;
        app.render.transcript_cache.take_work_stats();
        app.preview.select(Some(deferred("a", "a", 1)));
        app.preview.select(Some(deferred("b", "b", 1)));
        let work = app.render.transcript_cache.take_work_stats();
        assert!(app.render.transcript_cache.valid);
        assert_eq!(work.rebuilds, 0);
        assert_eq!(work.patches, 0);
    }

    #[test]
    fn cached_target_is_reused_without_new_resolution() {
        let mut pane = PreviewPaneState::default();
        let request = pane.select(Some(deferred("a", "a", 1))).unwrap();
        pane.complete(
            request.request_id,
            request.key,
            request.revision,
            Ok(PreviewContent::Markdown("cached".into())),
        );
        pane.select(Some(deferred("b", "b", 1)));
        assert!(pane.select(Some(deferred("a", "a", 1))).is_none());
        assert_eq!(
            pane.state,
            PreviewState::Ready(PreviewContent::Markdown("cached".into()))
        );
    }
}
