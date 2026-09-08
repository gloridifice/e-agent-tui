//! Output-free execution values and pure inspection policy. Adapters own clocks and storage.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const TRACE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceIdentity {
    pub frontend: String,
    pub session_id: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Model,
    Read,
    Edit,
    Command,
    Search,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingSource {
    Backend,
    ClientObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasuredDuration {
    pub duration_ms: u64,
    pub source: TimingSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    Success,
    Failure,
    Cancelled,
}

/// Counts the observed normalized result, never requested read limits or a file's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedOutputLines {
    pub count: usize,
    pub truncated: bool,
}

impl ObservedOutputLines {
    pub fn from_output(output: &str, truncated: bool) -> Self {
        Self {
            count: output.lines().count(),
            truncated,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationSummary {
    Identity,
    Command { command: String },
    Paths { paths: Vec<String> },
    Search { query: String, path: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationStart {
    pub call_id: String,
    pub turn_id: Option<String>,
    pub parent_id: Option<String>,
    pub kind: OperationKind,
    pub name: String,
    pub summary: OperationSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationFinish {
    pub call_id: String,
    pub outcome: ExecutionOutcome,
    pub duration: Option<MeasuredDuration>,
    pub output_lines: Option<ObservedOutputLines>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "event",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExecutionEvent {
    Attached,
    Detached,
    TurnStarted {
        turn_id: String,
    },
    TurnFinished {
        turn_id: String,
        outcome: ExecutionOutcome,
    },
    Started(OperationStart),
    Finished(OperationFinish),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecord {
    pub sequence: u64,
    pub run_id: String,
    pub time_unix_ms: u64,
    pub event: ExecutionEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record_type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TraceLine {
    Header {
        version: u32,
        identity: TraceIdentity,
    },
    Event {
        record: ExecutionRecord,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionCall {
    pub sequence: u64,
    pub run_id: String,
    pub start_unix_ms: u64,
    pub end_unix_ms: Option<u64>,
    pub operation: OperationStart,
    pub finish: Option<OperationFinish>,
}

/// Only terminal, individually measured calls qualify; zero is a measurement.
pub fn longest_calls(calls: &[ExecutionCall], limit: usize) -> Vec<&ExecutionCall> {
    let mut measured: Vec<_> = calls
        .iter()
        .filter_map(|call| Some((call, call.finish.as_ref()?.duration?.duration_ms)))
        .collect();
    measured.sort_by(|(left, left_ms), (right, right_ms)| {
        right_ms
            .cmp(left_ms)
            .then_with(|| left.sequence.cmp(&right.sequence))
    });
    measured
        .into_iter()
        .take(limit)
        .map(|(call, _)| call)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryQueryKind {
    Path,
    Copy,
    CopyLongest10,
    Longest50,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryInputGuard {
    pub text: String,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryQueryRequest {
    pub request_id: u64,
    pub identity: TraceIdentity,
    pub kind: HistoryQueryKind,
    pub input_guard: Option<HistoryInputGuard>,
    pub after_offset: u64,
    pub watermark: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryQueryResult {
    pub path: String,
    pub records: Vec<ExecutionRecord>,
    pub ranked_calls: Vec<ExecutionCall>,
    pub warnings: Vec<String>,
    pub watermark: u64,
    pub next_offset: u64,
    pub has_more: bool,
}

pub fn calls_from_records(records: &[ExecutionRecord]) -> Vec<ExecutionCall> {
    let mut active: HashMap<(String, String), ExecutionCall> = HashMap::new();
    let mut calls = Vec::new();
    for record in records {
        match &record.event {
            ExecutionEvent::Started(operation) => {
                active.insert(
                    (record.run_id.clone(), operation.call_id.clone()),
                    ExecutionCall {
                        sequence: record.sequence,
                        run_id: record.run_id.clone(),
                        start_unix_ms: record.time_unix_ms,
                        end_unix_ms: None,
                        operation: operation.clone(),
                        finish: None,
                    },
                );
            }
            ExecutionEvent::Finished(finish) => {
                let key = (record.run_id.clone(), finish.call_id.clone());
                let mut call = active.remove(&key).unwrap_or_else(|| ExecutionCall {
                    sequence: record.sequence,
                    run_id: record.run_id.clone(),
                    start_unix_ms: record.time_unix_ms,
                    end_unix_ms: None,
                    operation: OperationStart {
                        call_id: finish.call_id.clone(),
                        turn_id: None,
                        parent_id: None,
                        kind: OperationKind::Other,
                        name: "unknown".into(),
                        summary: OperationSummary::Identity,
                    },
                    finish: None,
                });
                call.end_unix_ms = Some(record.time_unix_ms);
                call.finish = Some(finish.clone());
                calls.push(call);
            }
            _ => {}
        }
    }
    calls.extend(active.into_values());
    calls.sort_by(|left, right| {
        (left.start_unix_ms, left.sequence).cmp(&(right.start_unix_ms, right.sequence))
    });
    calls
}

fn summary_text(summary: &OperationSummary) -> String {
    match summary {
        OperationSummary::Identity => String::new(),
        OperationSummary::Command { command } => command.clone(),
        OperationSummary::Paths { paths } => paths.join(", "),
        OperationSummary::Search { query, path } => path
            .as_ref()
            .map_or_else(|| query.clone(), |path| format!("{query} at {path}")),
    }
}

pub fn format_history_export(
    records: &[ExecutionRecord],
    warnings: &[String],
    longest: Option<usize>,
) -> String {
    let calls = calls_from_records(records);
    let selected: Vec<&ExecutionCall> = match longest {
        Some(limit) => longest_calls(&calls, limit),
        None => calls.iter().collect(),
    };
    let mut output = String::from("Execution history\n");
    if selected.is_empty() {
        output.push_str("No eligible operations.\n");
    }
    for call in selected {
        let summary = summary_text(&call.operation.summary);
        let status = call
            .finish
            .as_ref()
            .map_or("running", |finish| match finish.outcome {
                ExecutionOutcome::Success => "success",
                ExecutionOutcome::Failure => "failure",
                ExecutionOutcome::Cancelled => "cancelled",
            });
        let duration = call
            .finish
            .as_ref()
            .and_then(|finish| finish.duration)
            .map_or_else(
                || "unknown".into(),
                |duration| format!("{}ms ({:?})", duration.duration_ms, duration.source),
            );
        let lines = call
            .finish
            .as_ref()
            .and_then(|finish| finish.output_lines)
            .map_or_else(
                || "--".into(),
                |lines| format!("{}{}", lines.count, if lines.truncated { "+" } else { "" }),
            );
        output.push_str(&format!(
            "{} {} {}{} | {} | {} | lines {}\n",
            call.start_unix_ms,
            call.operation.name,
            if summary.is_empty() { "" } else { &summary },
            call.end_unix_ms
                .map_or_else(String::new, |end| format!(" -> {end}")),
            status,
            duration,
            lines,
        ));
    }
    for warning in warnings {
        output.push_str(&format!("WARNING: {warning}\n"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{
            tool::{ActivityState, ToolActivity, ToolCapability, ToolReference},
            AgentEvent, TimelineEvent, TimelineFact, TimelineRecord,
        },
        execution_capture::{
            enrich_historical_tool_metrics, trace_tool_metrics, ExecutionCapture, ObservedAt,
        },
        preview::MutationHunk,
    };

    fn tool(capability: ToolCapability, reference: ToolReference) -> ToolActivity {
        ToolActivity {
            id: "call-1".into(),
            capability,
            label: "tool-name".into(),
            summary: "RAW_ARGUMENT_SECRET".into(),
            state: ActivityState::Running,
            reference: Some(reference),
            items: Vec::new(),
            preview: None,
        }
    }

    #[test]
    fn summaries_exclude_bodies_diffs_matches_and_unknown_arguments() {
        for (capability, reference) in [
            (
                ToolCapability::Read,
                ToolReference::Lines {
                    path: "a.rs".into(),
                    start: 1,
                    lines: vec!["FILE_BODY_SECRET".into()],
                },
            ),
            (
                ToolCapability::Edit,
                ToolReference::Diff {
                    path: Some("a.rs".into()),
                    diff: "DIFF_SECRET".into(),
                },
            ),
            (
                ToolCapability::Edit,
                ToolReference::Hunks(vec![MutationHunk {
                    path: Some("a.rs".into()),
                    old: Some("OLD_SECRET".into()),
                    new: Some("NEW_SECRET".into()),
                    anchor_line: None,
                }]),
            ),
            (
                ToolCapability::Search,
                ToolReference::SearchResult {
                    query: "needle".into(),
                    matches: vec!["MATCH_SECRET".into()],
                },
            ),
            (
                ToolCapability::Generic,
                ToolReference::Command {
                    command: "GENERIC_SECRET".into(),
                },
            ),
        ] {
            let start = OperationStart::from_tool(&tool(capability, reference), None, None);
            let serialized = serde_json::to_string(&start).unwrap();
            assert!(!serialized.contains("SECRET"), "{serialized}");
            assert_eq!(
                serde_json::from_str::<OperationStart>(&serialized).unwrap(),
                start
            );
        }
        let command = OperationStart::from_tool(
            &tool(
                ToolCapability::Command,
                ToolReference::Command {
                    command: "cargo test --locked".into(),
                },
            ),
            Some("turn-1".into()),
            None,
        );
        assert_eq!(
            command.summary,
            OperationSummary::Command {
                command: "cargo test --locked".into()
            }
        );
    }

    #[test]
    fn observed_lines_preserve_empty_truncated_and_line_ending_semantics() {
        assert_eq!(
            ObservedOutputLines::from_output("", false),
            ObservedOutputLines {
                count: 0,
                truncated: false
            }
        );
        assert_eq!(
            ObservedOutputLines::from_output("a\r\nb\r\n", true),
            ObservedOutputLines {
                count: 2,
                truncated: true
            }
        );
        assert_eq!(ObservedOutputLines::from_output("a\n\n", false).count, 2);
        assert_eq!(ObservedOutputLines::from_output("last", false).count, 1);
    }

    #[test]
    fn json_lines_round_trip_without_payload_fields() {
        let header = TraceLine::Header {
            version: TRACE_VERSION,
            identity: TraceIdentity {
                frontend: "test".into(),
                session_id: "s".into(),
                cwd: "/project/sub".into(),
            },
        };
        let end = TraceLine::Event {
            record: ExecutionRecord {
                sequence: 2,
                run_id: "r".into(),
                time_unix_ms: 123,
                event: ExecutionEvent::Finished(OperationFinish {
                    call_id: "c".into(),
                    outcome: ExecutionOutcome::Cancelled,
                    duration: None,
                    output_lines: None,
                }),
            },
        };
        for line in [header, end] {
            let json = serde_json::to_string(&line).unwrap();
            assert!(!json.contains('\n'));
            assert_eq!(serde_json::from_str::<TraceLine>(&json).unwrap(), line);
        }
        assert!(serde_json::from_str::<OperationFinish>(r#"{"call_id":"c","outcome":"success","duration":null,"output_lines":null,"stdout":"forbidden"}"#).is_err());
    }

    #[test]
    fn ingress_capture_prefers_backend_time_and_deduplicates_terminal_events() {
        use crate::agent::{
            timeline::{TimelineFact, TimelineRecord},
            AgentEvent, TimelineEvent,
        };
        let activity = tool(
            ToolCapability::Command,
            ToolReference::Command {
                command: "cargo check".into(),
            },
        );
        let event = |time_ms, fact| {
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                sequence: None,
                time_ms,
                surface: None,
                source_sequences: Vec::new(),
                fact,
            }))
        };
        let mut capture = ExecutionCapture::new("run".into());
        let at = |wall_unix_ms, monotonic_ms| ObservedAt {
            wall_unix_ms,
            monotonic_ms,
        };
        assert_eq!(
            capture
                .observe(&event(None, TimelineFact::TurnStart), at(1_000, 10))
                .len(),
            1
        );
        assert_eq!(
            capture
                .observe(
                    &event(Some(2_000), TimelineFact::ToolCall(activity.clone())),
                    at(2_001, 20)
                )
                .len(),
            1
        );
        let result = TimelineFact::ToolResult {
            activity_id: activity.id.clone(),
            output: "one\ntwo".into(),
            state: ActivityState::Failure,
            output_truncated: true,
            execution_metrics: None,
            starts_thinking: false,
            mutation_diff: None,
            mutation_hunks: Vec::new(),
        };
        let finished = capture.observe(&event(Some(2_125), result.clone()), at(2_130, 20));
        let ExecutionEvent::Finished(finish) = &finished[0].event else {
            panic!()
        };
        assert_eq!(
            finish.duration,
            Some(MeasuredDuration {
                duration_ms: 125,
                source: TimingSource::Backend
            })
        );
        assert_eq!(
            finish.output_lines,
            Some(ObservedOutputLines {
                count: 2,
                truncated: true
            })
        );
        assert_eq!(finish.outcome, ExecutionOutcome::Failure);
        assert!(capture
            .observe(&event(Some(2_126), result), at(2_131, 21))
            .is_empty());
        assert!(capture
            .observe(
                &AgentEvent::Timeline(TimelineEvent::Snapshot {
                    records: Vec::new(),
                    truncated: false
                }),
                at(3_000, 30)
            )
            .is_empty());
    }

    #[test]
    fn ingress_capture_uses_monotonic_fallback_and_keeps_missing_starts_unknown() {
        use crate::agent::{
            timeline::{TimelineFact, TimelineRecord},
            AgentEvent, TimelineEvent,
        };
        let event = |fact| {
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                sequence: None,
                time_ms: None,
                surface: None,
                source_sequences: Vec::new(),
                fact,
            }))
        };
        let at = |wall_unix_ms, monotonic_ms| ObservedAt {
            wall_unix_ms,
            monotonic_ms,
        };
        let mut capture = ExecutionCapture::new("run".into());
        let activity = tool(
            ToolCapability::Read,
            ToolReference::Path {
                path: "a.rs".into(),
            },
        );
        capture.observe(
            &event(TimelineFact::ToolCall(activity.clone())),
            at(10_000, 50),
        );
        let result = |id: &str| TimelineFact::ToolResult {
            activity_id: id.into(),
            output: String::new(),
            state: ActivityState::Success,
            output_truncated: false,
            execution_metrics: None,
            starts_thinking: false,
            mutation_diff: None,
            mutation_hunks: Vec::new(),
        };
        let finished = capture.observe(&event(result(&activity.id)), at(9_000, 50));
        let ExecutionEvent::Finished(finish) = &finished[0].event else {
            panic!()
        };
        assert_eq!(
            finish.duration,
            Some(MeasuredDuration {
                duration_ms: 0,
                source: TimingSource::ClientObserved
            })
        );
        let missing = capture.observe(&event(result("missing")), at(8_000, 60));
        let ExecutionEvent::Finished(finish) = &missing[0].event else {
            panic!()
        };
        assert_eq!(finish.duration, None);
        assert_eq!(
            capture
                .observe(
                    &event(TimelineFact::StepStart {
                        turn: Some(2),
                        step: Some(3)
                    }),
                    at(11_000, 70)
                )
                .len(),
            1
        );
        let model = capture.observe(
            &event(TimelineFact::StepEnd {
                turn: Some(2),
                step: Some(3),
            }),
            at(11_400, 470),
        );
        let ExecutionEvent::Finished(finish) = &model[0].event else {
            panic!()
        };
        assert_eq!(finish.duration.unwrap().duration_ms, 400);
    }

    #[test]
    fn rankings_are_bounded_stable_and_exclude_unmeasured_calls() {
        let operation = OperationStart::from_tool(
            &tool(
                ToolCapability::Command,
                ToolReference::Command {
                    command: "cargo test".into(),
                },
            ),
            None,
            None,
        );
        let mut calls: Vec<_> = (0..65)
            .map(|sequence| ExecutionCall {
                sequence,
                run_id: "r".into(),
                start_unix_ms: 100,
                end_unix_ms: Some(200),
                operation: operation.clone(),
                finish: Some(OperationFinish {
                    call_id: operation.call_id.clone(),
                    outcome: ExecutionOutcome::Failure,
                    duration: Some(MeasuredDuration {
                        duration_ms: sequence / 2,
                        source: TimingSource::ClientObserved,
                    }),
                    output_lines: None,
                }),
            })
            .collect();
        let ranked = longest_calls(&calls, 50);
        assert_eq!(ranked.len(), 50);
        assert_eq!(ranked[0].sequence, 64);
        assert_eq!(ranked[1].sequence, 62);
        assert_eq!(ranked[2].sequence, 63);
        assert_eq!(longest_calls(&calls, 100).len(), 65);
        calls[0].finish = None;
        calls[1].finish.as_mut().unwrap().duration = None;
        assert_eq!(longest_calls(&calls, 100).len(), 63);
        assert!(longest_calls(&calls, 0).is_empty());
        assert!(longest_calls(&[], 50).is_empty());
    }

    #[test]
    fn historical_results_use_matching_trace_metrics_and_keep_missing_duration_unknown() {
        let start = OperationStart {
            call_id: "matched".into(),
            turn_id: None,
            parent_id: None,
            kind: OperationKind::Read,
            name: "read".into(),
            summary: OperationSummary::Paths {
                paths: vec!["a.rs".into()],
            },
        };
        let trace = vec![
            ExecutionRecord {
                sequence: 1,
                run_id: "run".into(),
                time_unix_ms: 1_000,
                event: ExecutionEvent::Started(start.clone()),
            },
            ExecutionRecord {
                sequence: 2,
                run_id: "run".into(),
                time_unix_ms: 1_125,
                event: ExecutionEvent::Finished(OperationFinish {
                    call_id: start.call_id,
                    outcome: ExecutionOutcome::Success,
                    duration: Some(MeasuredDuration {
                        duration_ms: 120,
                        source: TimingSource::Backend,
                    }),
                    output_lines: Some(ObservedOutputLines {
                        count: 9,
                        truncated: true,
                    }),
                }),
            },
        ];
        let result = |id: &str| TimelineRecord {
            sequence: None,
            time_ms: None,
            surface: None,
            source_sequences: Vec::new(),
            fact: TimelineFact::ToolResult {
                activity_id: id.into(),
                output: "native\noutput".into(),
                state: ActivityState::Success,
                output_truncated: false,
                execution_metrics: None,
                starts_thinking: false,
                mutation_diff: None,
                mutation_hunks: Vec::new(),
            },
        };
        let mut event = AgentEvent::Timeline(TimelineEvent::Snapshot {
            records: vec![result("matched"), result("missing")],
            truncated: false,
        });
        enrich_historical_tool_metrics(&mut event, &trace_tool_metrics(&trace));
        let AgentEvent::Timeline(TimelineEvent::Snapshot { records, .. }) = event else {
            panic!()
        };
        let metrics: Vec<_> = records
            .into_iter()
            .map(|record| match record.fact {
                TimelineFact::ToolResult {
                    execution_metrics, ..
                } => execution_metrics.unwrap(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(metrics[0].duration_ms, Some(120));
        assert_eq!(metrics[0].output_lines, Some(9));
        assert!(metrics[0].output_lines_truncated);
        assert_eq!(metrics[1].duration_ms, None);
        assert_eq!(metrics[1].output_lines, Some(2));
    }
}
