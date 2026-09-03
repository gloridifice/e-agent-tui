//! Ordered execution of provider-neutral frontend actions.

use crate::{
    clipboard_preview,
    runtime::{AgentRequestPort, DirtyReason, FrameScheduler, UiActionPorts},
    DrawPriority, EffectResult, UiAction,
};

/// Results returned after executing one ordered batch of [`UiAction`] values.
///
/// `fatal` is deliberately a plain string: executable adapters retain their
/// provider-specific transport error types while the frontend only needs a
/// normalized terminal failure reason.
#[derive(Debug, Default)]
pub struct EffectExecution {
    pub completed: Vec<EffectResult>,
    pub quit: bool,
    pub fatal: Option<String>,
}

/// Execute actions in their original order through adapter-owned ports.
///
/// Callers must obtain the owned action vector while holding frontend state,
/// release that state guard, and only then await this function.
pub async fn execute_ui_actions(
    actions: Vec<UiAction>,
    agent: &mut impl AgentRequestPort,
    scheduler: &mut FrameScheduler,
    ports: &mut impl UiActionPorts,
) -> EffectExecution {
    let mut execution = EffectExecution::default();
    for action in actions {
        match action {
            UiAction::Agent(request) => {
                if let Err(error) = agent.send_agent_request(request).await {
                    execution.fatal = Some(error);
                    break;
                }
            }
            UiAction::ResolvePreview(request) => {
                let result = ports.resolve_preview(request.clone()).await;
                execution.completed.push(EffectResult::PreviewResolved {
                    request_id: request.request_id,
                    key: request.key,
                    revision: request.revision,
                    result,
                });
            }
            UiAction::PersistConfig(config) => execution
                .completed
                .push(EffectResult::ConfigPersisted(ports.persist_config(&config))),
            UiAction::ReloadConfig => execution.completed.push(match ports.load_config() {
                Ok((config, themes)) => EffectResult::ConfigReloaded {
                    config: Box::new(config),
                    themes,
                },
                Err(error) => EffectResult::ConfigReloadFailed(error),
            }),
            UiAction::PersistSessionId(session_id) => ports.persist_session_id(session_id),
            UiAction::ReadClipboard => execution
                .completed
                .push(EffectResult::ClipboardRead(ports.read_clipboard())),
            UiAction::WriteClipboard(text) => {
                let lines = text.lines().count();
                let (preview, truncated) = clipboard_preview(&text, 6);
                execution.completed.push(match ports.write_clipboard(text) {
                    Ok(()) => EffectResult::ClipboardWritten {
                        lines,
                        preview,
                        truncated,
                    },
                    Err(error) => EffectResult::ClipboardFailed(error),
                });
            }
            UiAction::RequestDraw(priority) => scheduler.request(
                match priority {
                    DrawPriority::Interactive => DirtyReason::Interactive,
                    DrawPriority::Content => DirtyReason::Content,
                    DrawPriority::Animation => DirtyReason::Animation,
                },
                ports.now(),
            ),
            UiAction::Quit => execution.quit = true,
            UiAction::Fatal(reason) => {
                execution.fatal = Some(reason);
                break;
            }
        }
    }
    execution
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        sync::{Arc, Mutex},
        time::Instant,
    };

    use super::*;
    use crate::{
        runtime::{ScriptedAgentRequestPort, ScriptedUiActionPorts},
        AgentRequest, ClipboardPaste, Config, PreviewContent, PreviewKey, PreviewRequest,
        PreviewRequestId, PreviewRevision, ThemeFile,
    };

    struct RecordingAgent {
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl AgentRequestPort for RecordingAgent {
        fn send_agent_request(
            &mut self,
            _request: AgentRequest,
        ) -> impl Future<Output = Result<(), String>> + Send {
            self.calls.lock().unwrap().push("agent");
            std::future::ready(Ok(()))
        }
    }

    struct LockCheckingAgent {
        frontend_lock: Arc<Mutex<()>>,
        requests: Vec<AgentRequest>,
    }

    impl AgentRequestPort for LockCheckingAgent {
        fn send_agent_request(
            &mut self,
            request: AgentRequest,
        ) -> impl Future<Output = Result<(), String>> + Send {
            self.requests.push(request);
            let error = if self.frontend_lock.try_lock().is_ok() {
                "transport closed"
            } else {
                "frontend state was still locked"
            };
            std::future::ready(Err(error.into()))
        }
    }

    struct RecordingPorts {
        calls: Arc<Mutex<Vec<&'static str>>>,
        now: Instant,
    }

    impl UiActionPorts for RecordingPorts {
        fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String> {
            Err("not used".into())
        }

        fn persist_config(&mut self, _config: &Config) -> Result<(), String> {
            self.calls.lock().unwrap().push("persist-config");
            Ok(())
        }

        fn persist_session_id(&mut self, _session_id: String) {
            self.calls.lock().unwrap().push("persist-session");
        }

        fn read_clipboard(&mut self) -> Result<ClipboardPaste, String> {
            Err("not used".into())
        }

        fn write_clipboard(&mut self, _text: String) -> Result<(), String> {
            self.calls.lock().unwrap().push("write-clipboard");
            Ok(())
        }

        fn resolve_preview(
            &mut self,
            _request: PreviewRequest,
        ) -> impl Future<Output = Result<PreviewContent, String>> + Send {
            self.calls.lock().unwrap().push("preview");
            std::future::ready(Ok(PreviewContent::PlainText("resolved".into())))
        }

        fn now(&self) -> Instant {
            self.now
        }
    }

    #[tokio::test]
    async fn executor_preserves_interleaved_agent_and_effect_order() {
        let now = Instant::now();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut agent = RecordingAgent {
            calls: Arc::clone(&calls),
        };
        let mut ports = RecordingPorts {
            calls: Arc::clone(&calls),
            now,
        };
        let mut scheduler = FrameScheduler::new(now);
        let execution = execute_ui_actions(
            vec![
                UiAction::Agent(AgentRequest::Interrupt),
                UiAction::PersistSessionId("session-1".into()),
                UiAction::PersistConfig(Config::default()),
                UiAction::WriteClipboard("copy".into()),
                UiAction::ResolvePreview(PreviewRequest {
                    request_id: PreviewRequestId(4),
                    key: PreviewKey("file:test".into()),
                    revision: PreviewRevision(2),
                }),
                UiAction::RequestDraw(DrawPriority::Interactive),
                UiAction::Quit,
            ],
            &mut agent,
            &mut scheduler,
            &mut ports,
        )
        .await;

        assert_eq!(
            *calls.lock().unwrap(),
            [
                "agent",
                "persist-session",
                "persist-config",
                "write-clipboard",
                "preview"
            ]
        );
        assert_eq!(execution.completed.len(), 3);
        assert!(execution.quit);
        assert!(execution.fatal.is_none());
        assert_eq!(scheduler.take_due(now), Some(now));
    }

    #[tokio::test]
    async fn executor_returns_normalized_failures_and_preview_completion() {
        let now = Instant::now();
        let mut agent = ScriptedAgentRequestPort::default();
        let mut ports = ScriptedUiActionPorts::successful(now);
        ports.config_result = Err("persist denied".into());
        ports.clipboard_result = Err("clipboard denied".into());
        ports
            .preview_results
            .push_back(Ok(PreviewContent::PlainText("preview".into())));
        let mut scheduler = FrameScheduler::new(now);

        let execution = execute_ui_actions(
            vec![
                UiAction::PersistConfig(Config::default()),
                UiAction::WriteClipboard("copy".into()),
                UiAction::ResolvePreview(PreviewRequest {
                    request_id: PreviewRequestId(5),
                    key: PreviewKey("file:test".into()),
                    revision: PreviewRevision(3),
                }),
            ],
            &mut agent,
            &mut scheduler,
            &mut ports,
        )
        .await;

        assert!(matches!(
            execution.completed.as_slice(),
            [
                EffectResult::ConfigPersisted(Err(config_error)),
                EffectResult::ClipboardFailed(clipboard_error),
                EffectResult::PreviewResolved { result: Ok(PreviewContent::PlainText(text)), .. },
            ] if config_error == "persist denied" && clipboard_error == "clipboard denied" && text == "preview"
        ));
    }

    #[tokio::test]
    async fn executor_reports_transport_failure_after_callers_release_frontend_state() {
        let now = Instant::now();
        let frontend_lock = Arc::new(Mutex::new(()));
        let actions = {
            let _guard = frontend_lock.lock().unwrap();
            vec![UiAction::Agent(AgentRequest::Interrupt)]
        };
        let mut agent = LockCheckingAgent {
            frontend_lock: Arc::clone(&frontend_lock),
            requests: Vec::new(),
        };
        let mut ports = ScriptedUiActionPorts::successful(now);
        let mut scheduler = FrameScheduler::new(now);

        let execution = execute_ui_actions(actions, &mut agent, &mut scheduler, &mut ports).await;
        assert!(frontend_lock.try_lock().is_ok());
        assert_eq!(agent.requests, [AgentRequest::Interrupt]);
        assert_eq!(execution.fatal.as_deref(), Some("transport closed"));
        assert!(execution.completed.is_empty());
    }
}
