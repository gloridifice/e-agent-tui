//! Provider-neutral demand and completion values for incremental session discovery.

mod tree;
pub use tree::{session_tree, SessionParents, SessionTreeRow};

use crate::agent::SessionSummary;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

pub const MAX_RESUME_BATCH_SIZE: usize = 3;

pub(crate) fn relative_age(modified: SystemTime, now: SystemTime) -> (String, Duration) {
    let elapsed = match now.duration_since(modified) {
        Ok(elapsed) => elapsed,
        Err(future) => {
            return (
                "0s".into(),
                future.duration().saturating_add(Duration::from_secs(1)),
            );
        }
    };
    let seconds = elapsed.as_secs();
    let (major, major_unit, minor, minor_unit) = match seconds {
        86_400.. => (86_400, "d", 3_600, "h"),
        3_600.. => (3_600, "h", 60, "m"),
        60.. => (60, "m", 1, "s"),
        _ => (1, "s", 1, "s"),
    };
    let mut label = format!("{}{major_unit}", seconds / major);
    let remainder = seconds % major / minor;
    if remainder > 0 {
        label.push_str(&format!("{remainder}{minor_unit}"));
    }
    let refresh_in = Duration::from_secs(minor - seconds % minor)
        - Duration::from_nanos(u64::from(elapsed.subsec_nanos()));
    (label, refresh_in)
}

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
                (limit - loaded).min(MAX_RESUME_BATCH_SIZE)
            } else {
                MAX_RESUME_BATCH_SIZE
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn relative_ages_use_integer_adjacent_units_and_exact_refresh_boundaries() {
        for (seconds, label, refresh_seconds) in [
            (0, "0s", 1),
            (5, "5s", 1),
            (59, "59s", 1),
            (60, "1m", 1),
            (61, "1m1s", 1),
            (600, "10m", 1),
            (3_599, "59m59s", 1),
            (3_600, "1h", 60),
            (3_661, "1h1m", 59),
            (46_800, "13h", 60),
            (86_399, "23h59m", 1),
            (86_400, "1d", 3_600),
            (86_461, "1d", 3_539),
            (266_400, "3d2h", 3_600),
            (31_536_000, "365d", 3_600),
        ] {
            let now = UNIX_EPOCH + Duration::from_secs(seconds) + Duration::from_millis(250);
            let (actual, refresh) = relative_age(UNIX_EPOCH, now);
            assert_eq!(actual, label);
            assert_eq!(
                refresh,
                Duration::from_secs(refresh_seconds) - Duration::from_millis(250)
            );
            assert_eq!(
                relative_age(UNIX_EPOCH, now + refresh - Duration::from_millis(1)).0,
                label
            );
            assert_ne!(relative_age(UNIX_EPOCH, now + refresh).0, label);
        }
    }

    #[test]
    fn relative_age_clamps_future_modifications_until_one_second_old() {
        let modified = UNIX_EPOCH + Duration::from_secs(5);
        let (label, refresh) = relative_age(modified, UNIX_EPOCH);
        assert_eq!(label, "0s");
        assert_eq!(refresh, Duration::from_secs(6));
        assert_eq!(relative_age(modified, UNIX_EPOCH + refresh).0, "1s");
    }
}
