//! Provider-neutral demand and completion values for incremental session discovery.

use crate::agent::SessionSummary;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeRequest {
    pub generation: u64,
    pub workspace: String,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug)]
pub struct ResumeBatch {
    pub request: ResumeRequest,
    pub sessions: Vec<SessionSummary>,
    pub next_offset: usize,
    pub has_more: bool,
    pub diagnostic: Option<String>,
}

pub struct ResumePaging {
    generation: u64,
    workspace: Option<String>,
    offset: usize,
    pending: Option<ResumeRequest>,
    pub has_more: bool,
    pub visible_rows: usize,
    pub diagnostic: Option<String>,
}

impl Default for ResumePaging {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            generation: NEXT.fetch_add(1, Ordering::Relaxed),
            workspace: None,
            offset: 0,
            pending: None,
            has_more: true,
            visible_rows: 0,
            diagnostic: None,
        }
    }
}

impl ResumePaging {
    pub fn bind_workspace(&mut self, workspace: &str) -> bool {
        if self.workspace.as_deref() == Some(workspace) {
            return false;
        }
        let rows = self.visible_rows;
        *self = Self::default();
        self.visible_rows = rows;
        self.workspace = Some(workspace.to_owned());
        true
    }

    pub fn request(
        &mut self,
        selected: usize,
        loaded: usize,
        searching: bool,
    ) -> Option<ResumeRequest> {
        if self.visible_rows == 0 || self.pending.is_some() || !self.has_more {
            return None;
        }
        let limit = self.visible_rows.saturating_mul(2);
        if !searching && loaded >= limit && selected.saturating_add(self.visible_rows) < loaded {
            return None;
        }
        let request = ResumeRequest {
            generation: self.generation,
            workspace: self.workspace.clone()?,
            offset: self.offset,
            limit: if !searching && loaded < limit {
                limit - loaded
            } else {
                limit
            },
        };
        self.pending = Some(request.clone());
        Some(request)
    }

    pub fn admit(&mut self, batch: &ResumeBatch, workspace: &str) -> bool {
        if batch.request.workspace != workspace || self.pending.as_ref() != Some(&batch.request) {
            return false;
        }
        self.pending = None;
        self.offset = batch.next_offset;
        self.has_more = batch.has_more;
        if batch.diagnostic.is_some() {
            self.diagnostic.clone_from(&batch.diagnostic);
        }
        true
    }

    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
}
