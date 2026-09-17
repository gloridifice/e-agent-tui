//! Agent-event capture and native-history enrichment over output-free execution records.

use std::collections::{HashMap, HashSet};

use crate::{
    agent::tool::{ToolActivity, ToolCapability, ToolReference},
    execution_history::{
        calls_from_records, ExecutionEvent, ExecutionOutcome, ExecutionRecord, HistoryMessageKind,
        MeasuredDuration, ModelIdentity, ObservedOutputLines, OperationFinish, OperationKind,
        OperationStart, OperationSummary, TimingSource, TokenUsageRecord,
    },
    preview::ToolPreviewPrimary,
};

impl OperationStart {
    /// Presentation summaries and generic Preview payloads may contain raw arguments.
    /// Only typed, allowlisted reference fields may enter the trace.
    pub fn from_tool(
        activity: &ToolActivity,
        turn_id: Option<String>,
        parent_id: Option<String>,
    ) -> Self {
        let kind = match activity.capability {
            ToolCapability::Read | ToolCapability::SkillRead | ToolCapability::View => {
                OperationKind::Read
            }
            ToolCapability::Edit
            | ToolCapability::Insert
            | ToolCapability::Replace
            | ToolCapability::Create => OperationKind::Edit,
            ToolCapability::Command => OperationKind::Command,
            ToolCapability::Search => OperationKind::Search,
            ToolCapability::Generic | ToolCapability::Custom { .. } => OperationKind::Other,
        };
        let summary = match kind {
            OperationKind::Command => match activity.reference.as_ref() {
                Some(ToolReference::Command { command }) => OperationSummary::Command {
                    command: command.clone(),
                },
                _ => OperationSummary::Identity,
            },
            OperationKind::Read | OperationKind::Edit => {
                let mut paths = match activity.reference.as_ref() {
                    Some(ToolReference::Path { path } | ToolReference::Lines { path, .. }) => {
                        vec![path.clone()]
                    }
                    Some(ToolReference::Diff { path, .. }) => path.iter().cloned().collect(),
                    Some(ToolReference::Hunks(hunks)) => {
                        hunks.iter().filter_map(|hunk| hunk.path.clone()).collect()
                    }
                    _ => Vec::new(),
                };
                paths.dedup();
                if paths.is_empty() {
                    OperationSummary::Identity
                } else {
                    OperationSummary::Paths { paths }
                }
            }
            OperationKind::Search => {
                match activity.preview.as_ref().map(|preview| &preview.primary) {
                    Some(ToolPreviewPrimary::Search { query, path }) => OperationSummary::Search {
                        query: query.clone(),
                        path: path.clone(),
                    },
                    _ => match activity.reference.as_ref() {
                        Some(ToolReference::SearchResult { query, .. }) => {
                            OperationSummary::Search {
                                query: query.clone(),
                                path: None,
                            }
                        }
                        _ => OperationSummary::Identity,
                    },
                }
            }
            OperationKind::Model | OperationKind::Other => OperationSummary::Identity,
        };
        Self {
            call_id: activity.id.clone(),
            turn_id,
            parent_id,
            kind,
            name: activity.label.clone(),
            summary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedAt {
    pub wall_unix_ms: u64,
    pub monotonic_ms: u64,
}

impl From<crate::agent::timeline::TokenUsage> for TokenUsageRecord {
    fn from(usage: crate::agent::timeline::TokenUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveOperation {
    backend_start_ms: Option<u64>,
    observed_start: ObservedAt,
}

/// Pure adapter-ingress state. Snapshot/history replay is intentionally ignored.
#[derive(Debug)]
pub struct ExecutionCapture {
    run_id: String,
    next_sequence: u64,
    next_turn: u64,
    next_model: u64,
    current_turn: Option<String>,
    active: HashMap<String, ActiveOperation>,
    active_models: Vec<String>,
    terminal: HashSet<String>,
    current_model: Option<ModelIdentity>,
    pending_cost_usd_nanos: Option<u64>,
    pending_usage: Option<TokenUsageRecord>,
}

impl ExecutionCapture {
    pub fn new(run_id: String) -> Self {
        Self {
            run_id,
            next_sequence: 1,
            next_turn: 1,
            next_model: 1,
            current_turn: None,
            active: HashMap::new(),
            active_models: Vec::new(),
            terminal: HashSet::new(),
            current_model: None,
            pending_cost_usd_nanos: None,
            pending_usage: None,
        }
    }

    pub fn set_initial_model(&mut self, provider: Option<&str>, model: Option<&str>) {
        self.current_model = match (provider, model) {
            (Some(provider), Some(model)) => Some(ModelIdentity {
                provider: provider.to_owned(),
                model: model.to_owned(),
            }),
            _ => None,
        };
    }

    pub fn attached(&mut self, at: ObservedAt) -> ExecutionRecord {
        self.record(at.wall_unix_ms, ExecutionEvent::Attached)
    }

    pub fn detached(&mut self, at: ObservedAt) -> ExecutionRecord {
        self.record(at.wall_unix_ms, ExecutionEvent::Detached)
    }

    pub fn observe(&mut self, event: &crate::AgentEvent, at: ObservedAt) -> Vec<ExecutionRecord> {
        use crate::agent::{timeline::TimelineFact, CatalogEvent, TimelineEvent};
        if let crate::AgentEvent::Catalog(CatalogEvent::Models {
            current: Some(current),
            ..
        }) = event
        {
            let model = ModelIdentity {
                provider: current.provider.clone(),
                model: current.model.clone(),
            };
            if self.current_model.as_ref() == Some(&model) {
                return Vec::new();
            }
            self.current_model = Some(model.clone());
            return vec![self.record(at.wall_unix_ms, ExecutionEvent::ModelSelected { model })];
        }
        let crate::AgentEvent::Timeline(TimelineEvent::Append(timeline)) = event else {
            return Vec::new();
        };
        let backend_time = timeline.time_ms;
        let record_time = backend_time.unwrap_or(at.wall_unix_ms);
        match &timeline.fact {
            TimelineFact::UserMessage { .. } => {
                let mut records = Vec::new();
                if self.current_turn.is_none() {
                    let turn_id = self.begin_turn();
                    records.push(self.record(record_time, ExecutionEvent::TurnStarted { turn_id }));
                }
                records.push(self.message(record_time, HistoryMessageKind::User));
                records
            }
            TimelineFact::TurnStart => {
                if self.current_turn.is_some() {
                    Vec::new()
                } else {
                    let turn_id = self.begin_turn();
                    vec![self.record(record_time, ExecutionEvent::TurnStarted { turn_id })]
                }
            }
            TimelineFact::TurnEnd { error_message, .. } => {
                let turn_id = self
                    .current_turn
                    .clone()
                    .unwrap_or_else(|| "turn-unknown".into());
                let outcome = if error_message.is_some() {
                    ExecutionOutcome::Failure
                } else {
                    ExecutionOutcome::Success
                };
                let records = vec![
                    self.message(record_time, HistoryMessageKind::AgentStop),
                    self.record(
                        record_time,
                        ExecutionEvent::TurnFinished {
                            turn_id: turn_id.clone(),
                            outcome,
                        },
                    ),
                ];
                self.current_turn = None;
                self.pending_usage = None;
                self.pending_cost_usd_nanos = None;
                records
            }
            TimelineFact::UsageCost { usd_nanos } => {
                self.pending_cost_usd_nanos = Some(*usd_nanos);
                Vec::new()
            }
            TimelineFact::AssistantChunk {
                usage: Some(usage), ..
            } => {
                self.pending_usage = Some(TokenUsageRecord::from(*usage));
                Vec::new()
            }
            TimelineFact::AssistantMessage { usage, .. } => {
                let mut records = vec![self.message(record_time, HistoryMessageKind::Assistant)];
                let cost_usd_nanos = self.pending_cost_usd_nanos.take();
                let usage = usage
                    .map(TokenUsageRecord::from)
                    .or_else(|| self.pending_usage.take());
                self.pending_usage = None;
                if let Some(usage) = usage {
                    records.push(self.record(
                        record_time,
                        ExecutionEvent::UsageRecorded {
                            turn_id: self.current_turn.clone(),
                            model: self.current_model.clone(),
                            usage,
                            cost_usd_nanos,
                        },
                    ));
                }
                records
            }
            TimelineFact::ToolCall(activity) => {
                let operation =
                    OperationStart::from_tool(activity, self.current_turn.clone(), None);
                self.start(operation, backend_time, at, record_time)
            }
            TimelineFact::ToolResult {
                activity_id,
                output,
                state,
                output_truncated,
                ..
            } => {
                let outcome = match state {
                    crate::agent::tool::ActivityState::Failure => ExecutionOutcome::Failure,
                    crate::agent::tool::ActivityState::Cancelled => ExecutionOutcome::Cancelled,
                    _ => ExecutionOutcome::Success,
                };
                let lines = Some(ObservedOutputLines::from_output(output, *output_truncated));
                self.finish(activity_id, outcome, lines, backend_time, at, record_time)
            }
            TimelineFact::StepStart { turn, step } => {
                let call_id = format!(
                    "model:{}:{}:{}",
                    turn.unwrap_or_default(),
                    step.unwrap_or_default(),
                    self.next_model
                );
                self.next_model = self.next_model.saturating_add(1);
                let operation = OperationStart {
                    call_id,
                    turn_id: self
                        .current_turn
                        .clone()
                        .or_else(|| turn.map(|turn| format!("turn-{turn}"))),
                    parent_id: None,
                    kind: OperationKind::Model,
                    name: "model".into(),
                    summary: OperationSummary::Identity,
                };
                self.active_models.push(operation.call_id.clone());
                self.start(operation, backend_time, at, record_time)
            }
            TimelineFact::StepEnd { turn, step } => {
                let prefix = format!(
                    "model:{}:{}:",
                    turn.unwrap_or_default(),
                    step.unwrap_or_default()
                );
                let id = self
                    .active_models
                    .iter()
                    .rposition(|id| id.starts_with(&prefix))
                    .map(|index| self.active_models.remove(index));
                id.map(|id| {
                    self.finish(
                        &id,
                        ExecutionOutcome::Success,
                        None,
                        backend_time,
                        at,
                        record_time,
                    )
                })
                .unwrap_or_default()
            }
            TimelineFact::CommandStarted { id, name, args } => {
                let operation = OperationStart {
                    call_id: id.clone(),
                    turn_id: self.current_turn.clone(),
                    parent_id: None,
                    kind: OperationKind::Command,
                    name: name.clone(),
                    summary: args.as_ref().map_or(OperationSummary::Identity, |command| {
                        OperationSummary::Command {
                            command: command.clone(),
                        }
                    }),
                };
                self.start(operation, backend_time, at, record_time)
            }
            TimelineFact::CommandFinished { id, success, .. } => self.finish(
                id,
                if *success {
                    ExecutionOutcome::Success
                } else {
                    ExecutionOutcome::Failure
                },
                None,
                backend_time,
                at,
                record_time,
            ),
            _ => Vec::new(),
        }
    }

    fn begin_turn(&mut self) -> String {
        let turn_id = format!("turn-{}", self.next_turn);
        self.next_turn = self.next_turn.saturating_add(1);
        self.current_turn = Some(turn_id.clone());
        turn_id
    }

    fn message(&mut self, time_unix_ms: u64, kind: HistoryMessageKind) -> ExecutionRecord {
        self.record(
            time_unix_ms,
            ExecutionEvent::MessageObserved {
                turn_id: self.current_turn.clone(),
                kind,
                model: self.current_model.clone(),
            },
        )
    }

    fn start(
        &mut self,
        operation: OperationStart,
        backend_start_ms: Option<u64>,
        at: ObservedAt,
        record_time: u64,
    ) -> Vec<ExecutionRecord> {
        if self.active.contains_key(&operation.call_id)
            || self.terminal.contains(&operation.call_id)
        {
            return Vec::new();
        }
        self.active.insert(
            operation.call_id.clone(),
            ActiveOperation {
                backend_start_ms,
                observed_start: at,
            },
        );
        vec![self.record(record_time, ExecutionEvent::Started(operation))]
    }

    fn finish(
        &mut self,
        call_id: &str,
        outcome: ExecutionOutcome,
        output_lines: Option<ObservedOutputLines>,
        backend_end_ms: Option<u64>,
        at: ObservedAt,
        record_time: u64,
    ) -> Vec<ExecutionRecord> {
        if !self.terminal.insert(call_id.to_owned()) {
            return Vec::new();
        }
        let duration = self.active.remove(call_id).map(|active| {
            match (active.backend_start_ms, backend_end_ms) {
                (Some(start), Some(end)) if end >= start => MeasuredDuration {
                    duration_ms: end - start,
                    source: TimingSource::Backend,
                },
                _ => MeasuredDuration {
                    duration_ms: at
                        .monotonic_ms
                        .saturating_sub(active.observed_start.monotonic_ms),
                    source: TimingSource::ClientObserved,
                },
            }
        });
        vec![self.record(
            record_time,
            ExecutionEvent::Finished(OperationFinish {
                call_id: call_id.to_owned(),
                outcome,
                duration,
                output_lines,
            }),
        )]
    }

    fn record(&mut self, time_unix_ms: u64, event: ExecutionEvent) -> ExecutionRecord {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        ExecutionRecord {
            sequence,
            run_id: self.run_id.clone(),
            time_unix_ms,
            event,
        }
    }
}

pub fn trace_tool_metrics(
    records: &[ExecutionRecord],
) -> HashMap<String, crate::agent::timeline::ToolExecutionMetrics> {
    calls_from_records(records)
        .into_iter()
        .filter_map(|call| {
            let finish = call.finish?;
            let duration_ms = finish.duration.map(|duration| duration.duration_ms);
            let output = finish.output_lines;
            Some((
                call.operation.call_id,
                crate::agent::timeline::ToolExecutionMetrics {
                    duration_ms,
                    output_lines: output.map(|lines| lines.count),
                    output_lines_truncated: output.is_some_and(|lines| lines.truncated),
                    started_unix_ms: Some(call.start_unix_ms),
                    ended_unix_ms: call.end_unix_ms,
                },
            ))
        })
        .collect()
}

pub fn enrich_historical_tool_metrics(
    event: &mut crate::agent::AgentEvent,
    trace_metrics: &HashMap<String, crate::agent::timeline::ToolExecutionMetrics>,
) {
    let records = match event {
        crate::agent::AgentEvent::Timeline(crate::agent::TimelineEvent::Snapshot {
            records,
            ..
        })
        | crate::agent::AgentEvent::Timeline(crate::agent::TimelineEvent::History {
            records,
            ..
        }) => records,
        _ => return,
    };
    for record in records {
        let ExecutionEventMetrics {
            activity_id,
            output,
            output_truncated,
            execution_metrics,
        } = match &mut record.fact {
            crate::agent::timeline::TimelineFact::ToolResult {
                activity_id,
                output,
                output_truncated,
                execution_metrics,
                ..
            } => ExecutionEventMetrics {
                activity_id,
                output,
                output_truncated,
                execution_metrics,
            },
            _ => continue,
        };
        let observed = ObservedOutputLines::from_output(output, *output_truncated);
        *execution_metrics = Some(trace_metrics.get(activity_id).copied().unwrap_or(
            crate::agent::timeline::ToolExecutionMetrics {
                duration_ms: None,
                output_lines: Some(observed.count),
                output_lines_truncated: observed.truncated,
                started_unix_ms: None,
                ended_unix_ms: None,
            },
        ));
    }
}

struct ExecutionEventMetrics<'a> {
    activity_id: &'a mut String,
    output: &'a mut String,
    output_truncated: &'a mut bool,
    execution_metrics: &'a mut Option<crate::agent::timeline::ToolExecutionMetrics>,
}
