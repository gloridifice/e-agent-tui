//! Session-scoped state for the full-screen execution-history page.

use crate::execution_history::{
    calls_from_records, ExecutionCall, ExecutionRecord, HistoryQueryResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryView {
    Turns,
    Longest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryLoadState {
    Loading,
    Ready,
    Empty,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct HistoryPage {
    pub request_id: u64,
    pub session_id: String,
    pub cwd: String,
    pub state: HistoryLoadState,
    pub records: Vec<ExecutionRecord>,
    pub calls: Vec<ExecutionCall>,
    pub longest_calls: Option<Vec<ExecutionCall>>,
    pub warnings: Vec<String>,
    pub watermark: Option<u64>,
    pub next_offset: u64,
    pub has_more: bool,
    pub loading_more: bool,
    pub view: HistoryView,
    pub turns_offset: usize,
    pub longest_offset: usize,
    pub body_height: usize,
}

impl HistoryPage {
    pub fn loading(request_id: u64, session_id: String, cwd: String) -> Self {
        Self {
            request_id,
            session_id,
            cwd,
            state: HistoryLoadState::Loading,
            records: Vec::new(),
            calls: Vec::new(),
            longest_calls: None,
            warnings: Vec::new(),
            watermark: None,
            next_offset: 0,
            has_more: false,
            loading_more: false,
            view: HistoryView::Turns,
            turns_offset: 0,
            longest_offset: 0,
            body_height: 1,
        }
    }

    pub fn complete(
        &mut self,
        request_id: u64,
        kind: crate::execution_history::HistoryQueryKind,
        mut result: HistoryQueryResult,
    ) -> bool {
        if self.request_id != request_id {
            return false;
        }
        if kind == crate::execution_history::HistoryQueryKind::Longest50 {
            self.longest_calls = Some(result.ranked_calls);
            self.loading_more = false;
            return true;
        }
        if result.next_offset > 0 && !self.records.is_empty() {
            self.records.append(&mut result.records);
            self.warnings.append(&mut result.warnings);
            self.warnings.sort();
            self.warnings.dedup();
        } else {
            self.records = result.records;
            self.warnings = result.warnings;
        }
        self.calls = calls_from_records(&self.records);
        self.watermark = Some(result.watermark);
        self.next_offset = result.next_offset;
        self.has_more = result.has_more;
        self.loading_more = false;
        self.state = if self.calls.is_empty() {
            HistoryLoadState::Empty
        } else {
            HistoryLoadState::Ready
        };
        true
    }

    pub fn fail(&mut self, request_id: u64, error: String) -> bool {
        if self.request_id != request_id {
            return false;
        }
        self.loading_more = false;
        self.state = HistoryLoadState::Error(error);
        true
    }

    pub fn offset(&self) -> usize {
        match self.view {
            HistoryView::Turns => self.turns_offset,
            HistoryView::Longest => self.longest_offset,
        }
    }

    pub fn set_offset(&mut self, value: usize) {
        match self.view {
            HistoryView::Turns => self.turns_offset = value,
            HistoryView::Longest => self.longest_offset = value,
        }
    }

    pub fn toggle(&mut self) {
        self.view = match self.view {
            HistoryView::Turns => HistoryView::Longest,
            HistoryView::Longest => HistoryView::Turns,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_history::{ExecutionEvent, HistoryQueryResult};

    #[test]
    fn stale_result_is_rejected_and_views_keep_independent_offsets() {
        let mut page = HistoryPage::loading(4, "session".into(), "root".into());
        let stale = HistoryQueryResult {
            path: "trace".into(),
            records: Vec::new(),
            ranked_calls: Vec::new(),
            warnings: Vec::new(),
            watermark: 1,
            next_offset: 1,
            has_more: false,
        };
        assert!(!page.complete(3, crate::execution_history::HistoryQueryKind::Show, stale,));
        page.set_offset(7);
        page.toggle();
        assert_eq!(page.offset(), 0);
        page.set_offset(11);
        page.toggle();
        assert_eq!(page.offset(), 7);
        page.toggle();
        assert_eq!(page.offset(), 11);
    }

    #[test]
    fn empty_result_has_a_distinct_state() {
        let mut page = HistoryPage::loading(1, "session".into(), "root".into());
        assert!(page.complete(
            1,
            crate::execution_history::HistoryQueryKind::Show,
            HistoryQueryResult {
                path: "trace".into(),
                records: vec![crate::execution_history::ExecutionRecord {
                    sequence: 1,
                    run_id: "run".into(),
                    time_unix_ms: 1,
                    event: ExecutionEvent::Attached,
                }],
                ranked_calls: Vec::new(),
                warnings: Vec::new(),
                watermark: 1,
                next_offset: 1,
                has_more: false,
            }
        ));
        assert_eq!(page.state, HistoryLoadState::Empty);
    }
}
