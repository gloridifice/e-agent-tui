//! Session-scoped state for the execution-history ranking page.

use crate::execution_history::{ExecutionCall, HistoryQueryResult};

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
    pub calls: Vec<ExecutionCall>,
    pub warnings: Vec<String>,
    offset: usize,
    pub body_height: usize,
}

impl HistoryPage {
    pub fn loading(request_id: u64, session_id: String, cwd: String) -> Self {
        Self {
            request_id,
            session_id,
            cwd,
            state: HistoryLoadState::Loading,
            calls: Vec::new(),
            warnings: Vec::new(),
            offset: 0,
            body_height: 1,
        }
    }

    pub fn complete(&mut self, request_id: u64, result: HistoryQueryResult) -> bool {
        if self.request_id != request_id {
            return false;
        }
        self.calls = result.ranked_calls;
        self.warnings = result.warnings;
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
        self.state = HistoryLoadState::Error(error);
        true
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn set_offset(&mut self, value: usize) {
        self.offset = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_history::{
        ExecutionOutcome, MeasuredDuration, OperationFinish, OperationKind, OperationStart,
        OperationSummary, TimingSource,
    };

    fn result(calls: Vec<ExecutionCall>) -> HistoryQueryResult {
        HistoryQueryResult {
            path: "trace".into(),
            records: Vec::new(),
            ranked_calls: calls,
            warnings: vec!["capture incomplete".into()],
            watermark: 1,
            next_offset: 1,
            has_more: false,
        }
    }

    #[test]
    fn stale_result_and_error_preserve_page_state_and_offset() {
        let mut page = HistoryPage::loading(4, "session".into(), "root".into());
        page.set_offset(7);
        assert!(!page.complete(3, result(Vec::new())));
        assert!(!page.fail(3, "stale error".into()));
        assert_eq!(page.state, HistoryLoadState::Loading);
        assert!(page.warnings.is_empty());
        assert_eq!(page.offset(), 7);
    }

    #[test]
    fn ranked_result_finishes_loading_without_chronological_records() {
        let call = ExecutionCall {
            sequence: 2,
            run_id: "run".into(),
            start_unix_ms: 1,
            end_unix_ms: Some(3),
            operation: OperationStart {
                call_id: "call".into(),
                turn_id: None,
                parent_id: None,
                kind: OperationKind::Command,
                name: "bash".into(),
                summary: OperationSummary::Identity,
            },
            finish: Some(OperationFinish {
                call_id: "call".into(),
                outcome: ExecutionOutcome::Success,
                duration: Some(MeasuredDuration {
                    duration_ms: 2,
                    source: TimingSource::Backend,
                }),
                output_lines: None,
            }),
        };
        let mut page = HistoryPage::loading(1, "session".into(), "root".into());
        assert!(page.complete(1, result(vec![call.clone()])));
        assert_eq!(page.state, HistoryLoadState::Ready);
        assert_eq!(page.calls, vec![call]);
        assert_eq!(page.warnings, ["capture incomplete"]);
    }

    #[test]
    fn empty_result_retains_capture_warnings() {
        let mut page = HistoryPage::loading(1, "session".into(), "root".into());
        assert!(page.complete(1, result(Vec::new())));
        assert_eq!(page.state, HistoryLoadState::Empty);
        assert_eq!(page.warnings, ["capture incomplete"]);
    }
}
