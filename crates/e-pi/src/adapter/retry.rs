//! Recovery after Pi's native retry/compaction loop has fully settled.

use std::time::{Duration, Instant};

use e_tui::agent::{tool::ActivityState, TimelineEvent};

use super::{
    AdapterOutput, AgentEvent, AgentRequest, AgentStatus, PiAdapter, SessionEvent, TimelineFact,
    TimelineRecord,
};
use crate::protocol::{RpcCommand, RpcRecord};

const DELAYS_SECS: [u64; 5] = [60, 300, 900, 1800, 1800];
const CONTINUATION: &str = "Continue from where the previous attempt stopped after an error. Use the existing conversation and completed tool results; do not repeat completed work.";

#[derive(Default)]
pub(super) struct RetryState {
    enabled: bool,
    cancelled: bool,
    attempt: usize,
    error: Option<String>,
    activity_id: Option<String>,
    waiting: Option<Waiting>,
    prompt_id: Option<String>,
    state_id: Option<String>,
    starting: bool,
}

struct Waiting {
    due: Instant,
    next_tick: Instant,
}

impl RetryState {
    pub(super) fn busy(&self) -> bool {
        self.waiting.is_some() || self.starting
    }
}

impl PiAdapter {
    pub fn set_error_auto_retry(&mut self, enabled: bool) -> AdapterOutput {
        if self.retry.enabled == enabled {
            return AdapterOutput::default();
        }
        self.retry.enabled = enabled;
        if enabled {
            // Do not revive recovery cancelled earlier in the current run.
            AdapterOutput::default()
        } else {
            cancel(self, "Automatic retry disabled")
        }
    }

    pub fn retry_deadline(&self) -> Option<Instant> {
        self.retry.waiting.as_ref().map(|waiting| waiting.next_tick)
    }

    pub fn tick_retry(&mut self, now: Instant) -> AdapterOutput {
        let Some(waiting) = self.retry.waiting.as_mut() else {
            return AdapterOutput::default();
        };
        if now < waiting.next_tick {
            return AdapterOutput::default();
        }
        let remaining = waiting.due.saturating_duration_since(now);
        let blocked = self.configuration_request.is_some()
            || self.pending_compaction.is_some()
            || self.pending_fork.is_some()
            || self.pending_skill_prompt.is_some()
            || self.pending_queue.operation.is_some();
        if remaining.is_zero() && !blocked {
            self.retry.waiting = None;
            self.retry.starting = true;
            let id = self.request_id("retry-prompt");
            self.retry.prompt_id = Some(id.clone());
            let mut output = progress(self, ActivityState::Running, "Retrying".into());
            output.commands.push(RpcCommand::Prompt {
                id: Some(id),
                message: CONTINUATION.into(),
                streaming_behavior: None,
            });
            return output;
        }
        let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        waiting.next_tick = if seconds > 0 {
            now + remaining - Duration::from_secs(seconds - 1)
        } else {
            now + Duration::from_secs(1)
        };
        countdown(self, seconds)
    }
}

fn progress(adapter: &PiAdapter, state: ActivityState, message: String) -> AdapterOutput {
    let Some(id) = adapter.retry.activity_id.clone() else {
        return AdapterOutput::default();
    };
    AdapterOutput::event(AgentEvent::Timeline(TimelineEvent::Append(
        TimelineRecord {
            sequence: None,
            time_ms: None,
            surface: None,
            source_sequences: Vec::new(),
            fact: TimelineFact::RetryProgress {
                id,
                state,
                message: format!(
                    "{}/{} · {message}",
                    adapter.retry.attempt,
                    DELAYS_SECS.len()
                ),
            },
        },
    )))
}

fn countdown(adapter: &PiAdapter, seconds: u64) -> AdapterOutput {
    let wait = if seconds == 0 {
        "Waiting for pending controls".into()
    } else {
        format!("Retry in {:02}:{:02}", seconds / 60, seconds % 60)
    };
    progress(
        adapter,
        ActivityState::Waiting,
        format!(
            "{wait} · {}",
            adapter.retry.error.as_deref().unwrap_or("Pi model error")
        ),
    )
}

fn finish(adapter: &mut PiAdapter, state: ActivityState, message: &str) -> AdapterOutput {
    let output = progress(adapter, state, message.into());
    adapter.retry.activity_id = None;
    adapter.retry.waiting = None;
    adapter.retry.starting = false;
    output
}

pub(super) fn cancel(adapter: &mut PiAdapter, message: &str) -> AdapterOutput {
    let pending = adapter.retry.busy();
    adapter.retry.cancelled = true;
    adapter.retry.error = None;
    adapter.retry.prompt_id = None;
    adapter.retry.state_id = None;
    let mut output = finish(adapter, ActivityState::Cancelled, message);
    adapter.retry.attempt = 0;
    if pending && !adapter.is_streaming {
        output
            .events
            .push(AgentEvent::Session(SessionEvent::Status(AgentStatus::Idle)));
    }
    output
}

pub(super) fn before_request(adapter: &mut PiAdapter, request: &AgentRequest) -> AdapterOutput {
    match request {
        AgentRequest::Interrupt => cancel(adapter, "Retry cancelled"),
        AgentRequest::Input { .. }
        | AgentRequest::Steer { .. }
        | AgentRequest::NewInput { .. }
        | AgentRequest::Attach { .. }
        | AgentRequest::Command { .. } => {
            let output = cancel(adapter, "Retry superseded by user request");
            // Only a new model prompt opens a fresh error-recovery budget.
            adapter.retry.cancelled = !matches!(
                request,
                AgentRequest::Input { .. }
                    | AgentRequest::Steer { .. }
                    | AgentRequest::NewInput { .. }
                    | AgentRequest::Command { .. }
            );
            output
        }
        _ => AdapterOutput::default(),
    }
}

pub(super) fn started(adapter: &mut PiAdapter) -> AdapterOutput {
    let output = if adapter.retry.waiting.is_some() {
        let output = cancel(adapter, "Retry superseded by a new run");
        adapter.retry.cancelled = false;
        output
    } else {
        AdapterOutput::default()
    };
    adapter.retry.starting = false;
    adapter.retry.error = None;
    output
}

pub(super) fn message(adapter: &mut PiAdapter, message: &serde_json::Value) -> AdapterOutput {
    if message.get("role").and_then(serde_json::Value::as_str) != Some("assistant") {
        return AdapterOutput::default();
    }
    match message
        .get("stopReason")
        .and_then(serde_json::Value::as_str)
    {
        Some("error") if adapter.retry.enabled && !adapter.retry.cancelled => {
            adapter.retry.error = Some(
                message
                    .get("errorMessage")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Pi model error")
                    .chars()
                    .take(512)
                    .collect(),
            );
            AdapterOutput::default()
        }
        Some("aborted") => cancel(adapter, "Retry cancelled"),
        Some("error") => AdapterOutput::default(),
        _ => {
            adapter.retry.error = None;
            let output = finish(adapter, ActivityState::Success, "Retry succeeded");
            adapter.retry.attempt = 0;
            output
        }
    }
}

pub(super) fn settled(adapter: &mut PiAdapter, now: Instant) -> AdapterOutput {
    if !adapter.retry.enabled
        || adapter.retry.cancelled
        || adapter.retry.error.is_none()
        || adapter.retry.busy()
    {
        return AdapterOutput::default();
    }
    let Some(delay) = DELAYS_SECS.get(adapter.retry.attempt).copied() else {
        let error = adapter.retry.error.take().unwrap_or_default();
        adapter.retry.cancelled = true;
        return finish(
            adapter,
            ActivityState::Failure,
            &format!("Retries exhausted · {error}"),
        );
    };
    adapter.retry.attempt += 1;
    if adapter.retry.activity_id.is_none() {
        adapter.retry.activity_id = Some(adapter.request_id("fallback-retry"));
    }
    adapter.retry.waiting = Some(Waiting {
        due: now + Duration::from_secs(delay),
        next_tick: now + Duration::from_secs(1),
    });
    countdown(adapter, delay)
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    let id = record.string("id")?;
    if id.starts_with("pie-retry-state-") {
        if adapter.retry.state_id.as_deref() != Some(id) {
            return Some(AdapterOutput::default());
        }
        adapter.retry.state_id = None;
        if !adapter.retry.starting {
            return Some(AdapterOutput::default());
        }
        if record.bool("success") == Some(true)
            && record
                .field("data")
                .and_then(|data| data.get("isStreaming"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        {
            adapter.is_streaming = true;
            adapter.retry.starting = false;
            return Some(AdapterOutput::default());
        }
        return Some(cancel(adapter, "Continuation did not start a model run"));
    }
    if !id.starts_with("pie-retry-prompt-") {
        return None;
    }
    if adapter.retry.prompt_id.as_deref() != Some(id) {
        return Some(AdapterOutput::default());
    }
    adapter.retry.prompt_id = None;
    if record.bool("success") == Some(true) {
        if adapter.retry.starting {
            let id = adapter.request_id("retry-state");
            adapter.retry.state_id = Some(id.clone());
            return Some(AdapterOutput::command(RpcCommand::GetState {
                id: Some(id),
            }));
        }
        return Some(AdapterOutput::default());
    }
    adapter.retry.cancelled = true;
    adapter.retry.error = None;
    let mut output = finish(
        adapter,
        ActivityState::Failure,
        &format!(
            "Retry rejected · {}",
            record.string("error").unwrap_or("Pi rejected continuation")
        ),
    );
    if !adapter.is_streaming {
        output
            .events
            .push(AgentEvent::Session(SessionEvent::Status(AgentStatus::Idle)));
    }
    Some(output)
}

pub(super) fn reset(adapter: &mut PiAdapter) {
    adapter.retry = RetryState {
        enabled: adapter.retry.enabled,
        ..RetryState::default()
    };
}
