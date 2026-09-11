//! Narrow external-effect seams for executable adapters and scripted tests.

use std::{future::Future, time::Instant};

use crossterm::event::Event;

use crate::{AgentRequest, ClipboardPaste, Config, PreviewContent, PreviewRequest, ThemeFile};

pub trait TerminalEventPort {
    fn next_event(&mut self) -> impl Future<Output = Option<Result<Event, String>>> + Send;
}

pub trait TerminalLifecyclePort {
    fn restore_terminal(&mut self) -> Result<(), String>;
}

pub trait AgentRequestPort {
    fn send_agent_request(
        &mut self,
        request: AgentRequest,
    ) -> impl Future<Output = Result<(), String>> + Send;
}

pub trait UiActionPorts {
    fn validate_links(
        &mut self,
        request: &crate::link_copy::LinkValidationRequest,
    ) -> impl Future<Output = Vec<crate::link_copy::CandidateGroupValidation>> + Send {
        std::future::ready(
            request
                .groups
                .iter()
                .map(|group| crate::link_copy::CandidateGroupValidation {
                    alternatives: vec![
                        crate::link_copy::PathValidation::Rejected;
                        group.alternatives.len()
                    ],
                })
                .collect(),
        )
    }
    fn complete_paths(
        &mut self,
        request: &crate::path_completion::PathCompletionRequest,
    ) -> impl Future<Output = Vec<crate::path_completion::PathCandidate>> + Send;
    fn query_history(
        &mut self,
        request: crate::execution_history::HistoryQueryRequest,
    ) -> impl Future<Output = Result<crate::execution_history::HistoryQueryResult, String>> + Send
    {
        std::future::ready(Err(format!(
            "execution history is unavailable for session {}",
            request.identity.session_id
        )))
    }
    fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String>;
    fn persist_config(&mut self, config: &Config) -> Result<(), String>;
    fn persist_session_id(&mut self, session_id: String);
    fn read_clipboard(&mut self) -> Result<ClipboardPaste, String>;
    fn write_clipboard(&mut self, text: String) -> Result<(), String>;
    fn resolve_preview(
        &mut self,
        request: PreviewRequest,
    ) -> impl Future<Output = Result<PreviewContent, String>> + Send;
    fn now(&self) -> Instant;
}

/// Scriptable provider-neutral agent transport for executor tests.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct ScriptedAgentRequestPort {
    pub requests: Vec<AgentRequest>,
    pub failure: Option<String>,
}

#[cfg(any(test, feature = "test-support"))]
impl AgentRequestPort for ScriptedAgentRequestPort {
    fn send_agent_request(
        &mut self,
        request: AgentRequest,
    ) -> impl Future<Output = Result<(), String>> + Send {
        self.requests.push(request);
        std::future::ready(match &self.failure {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        })
    }
}

/// Scriptable normalized effect and clock implementation shared by adapter tests.
#[cfg(any(test, feature = "test-support"))]
pub struct ScriptedUiActionPorts {
    pub loaded_config: Option<(Config, Vec<ThemeFile>)>,
    pub persisted_configs: usize,
    pub session_ids: Vec<String>,
    pub clipboard_reads: usize,
    pub clipboard_writes: Vec<String>,
    pub config_result: Result<(), String>,
    pub clipboard_read_result: Result<ClipboardPaste, String>,
    pub clipboard_result: Result<(), String>,
    pub preview_results: std::collections::VecDeque<Result<PreviewContent, String>>,
    pub preview_requests: Vec<PreviewRequest>,
    pub now: Instant,
}

#[cfg(any(test, feature = "test-support"))]
impl ScriptedUiActionPorts {
    pub fn successful(now: Instant) -> Self {
        Self {
            loaded_config: None,
            persisted_configs: 0,
            session_ids: Vec::new(),
            clipboard_reads: 0,
            clipboard_writes: Vec::new(),
            config_result: Ok(()),
            clipboard_read_result: Ok(ClipboardPaste::Text(String::new())),
            clipboard_result: Ok(()),
            preview_results: std::collections::VecDeque::new(),
            preview_requests: Vec::new(),
            now,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl UiActionPorts for ScriptedUiActionPorts {
    async fn complete_paths(
        &mut self,
        _request: &crate::path_completion::PathCompletionRequest,
    ) -> Vec<crate::path_completion::PathCandidate> {
        Vec::new()
    }
    fn query_history(
        &mut self,
        request: crate::execution_history::HistoryQueryRequest,
    ) -> impl Future<Output = Result<crate::execution_history::HistoryQueryResult, String>> + Send
    {
        std::future::ready(Err(format!(
            "no scripted history for {}",
            request.identity.session_id
        )))
    }

    fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String> {
        self.loaded_config
            .take()
            .ok_or_else(|| "scripted config load failed".into())
    }

    fn persist_config(&mut self, _config: &Config) -> Result<(), String> {
        self.persisted_configs += 1;
        self.config_result.clone()
    }

    fn persist_session_id(&mut self, session_id: String) {
        self.session_ids.push(session_id);
    }

    fn read_clipboard(&mut self) -> Result<ClipboardPaste, String> {
        self.clipboard_reads += 1;
        self.clipboard_read_result.clone()
    }

    fn write_clipboard(&mut self, text: String) -> Result<(), String> {
        self.clipboard_writes.push(text);
        self.clipboard_result.clone()
    }

    fn resolve_preview(
        &mut self,
        request: PreviewRequest,
    ) -> impl Future<Output = Result<PreviewContent, String>> + Send {
        self.preview_requests.push(request);
        std::future::ready(
            self.preview_results
                .pop_front()
                .unwrap_or_else(|| Err("scripted Preview resolver failed".into())),
        )
    }

    fn now(&self) -> Instant {
        self.now
    }
}

/// Deterministic terminal event source for adapter and runtime tests.
#[cfg(any(test, feature = "test-support"))]
pub struct ScriptedTerminalEvents {
    events: std::collections::VecDeque<Result<Event, String>>,
}

#[cfg(any(test, feature = "test-support"))]
impl ScriptedTerminalEvents {
    pub fn new(events: impl IntoIterator<Item = Result<Event, String>>) -> Self {
        Self {
            events: events.into_iter().collect(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl TerminalEventPort for ScriptedTerminalEvents {
    fn next_event(&mut self) -> impl Future<Output = Option<Result<Event, String>>> + Send {
        std::future::ready(self.events.pop_front())
    }
}

/// Observable terminal restoration fake for adapter and runtime tests.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct ScriptedTerminalLifecycle {
    pub restore_calls: usize,
    pub fail: bool,
}

#[cfg(any(test, feature = "test-support"))]
impl TerminalLifecyclePort for ScriptedTerminalLifecycle {
    fn restore_terminal(&mut self) -> Result<(), String> {
        self.restore_calls += 1;
        if self.fail {
            Err("scripted restore failed".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PreviewKey, PreviewRequestId, PreviewRevision};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[tokio::test]
    async fn scripted_ports_are_deterministic_and_record_normalized_payloads() {
        let event = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let mut terminal = ScriptedTerminalEvents::new([Ok(event.clone()), Err("closed".into())]);
        assert!(matches!(terminal.next_event().await, Some(Ok(next)) if next == event));
        assert!(matches!(terminal.next_event().await, Some(Err(error)) if error == "closed"));
        assert!(terminal.next_event().await.is_none());

        let now = Instant::now();
        let mut effects = ScriptedUiActionPorts::successful(now);
        let request = PreviewRequest {
            request_id: PreviewRequestId(1),
            key: PreviewKey("file:test".into()),
            revision: PreviewRevision(2),
        };
        effects
            .preview_results
            .push_back(Ok(PreviewContent::PlainText("resolved".into())));
        assert!(matches!(
            effects.resolve_preview(request.clone()).await,
            Ok(PreviewContent::PlainText(text)) if text == "resolved"
        ));
        assert_eq!(effects.preview_requests, [request]);
        assert_eq!(effects.now(), now);
    }

    #[test]
    fn scripted_terminal_restoration_records_success_and_failure() {
        let mut terminal = ScriptedTerminalLifecycle::default();
        terminal.restore_terminal().unwrap();
        terminal.fail = true;
        assert!(terminal.restore_terminal().is_err());
        assert_eq!(terminal.restore_calls, 2);
    }
}
