//! Socket-local correlation for the bridge's serialized model-refresh/pong fence.

use std::{collections::VecDeque, sync::Mutex};

use e_tui::{
    agent::{AgentEvent, CatalogEvent, InteractionEvent, ModelSelection, SessionEvent},
    AgentRequest,
};
use tokio::sync::mpsc;

use crate::{
    protocol::{ClientMessage, ServerMessage},
    runtime_ports::BridgeTransportPort,
};

#[derive(Default)]
struct Fence {
    record_id: Option<u64>,
    session_id: Option<String>,
    model: Option<ModelSelection>,
    error: Option<String>,
}

#[derive(Default)]
struct Correlation {
    session_id: Option<String>,
    pings: VecDeque<Fence>,
}

pub struct PromptStateTransport {
    outbound: mpsc::Sender<ClientMessage>,
    correlation: Mutex<Correlation>,
}

impl PromptStateTransport {
    pub fn new(outbound: mpsc::Sender<ClientMessage>) -> Self {
        Self {
            outbound,
            correlation: Mutex::new(Correlation::default()),
        }
    }

    pub fn normalize(&self, message: ServerMessage) -> AgentEvent {
        let event = super::adapter::normalize_server_message(message);
        let mut correlation = self.correlation.lock().unwrap();
        match &event {
            AgentEvent::Session(SessionEvent::Attached(session)) => {
                correlation.session_id = Some(session.id.clone());
                correlation.pings.clear();
            }
            AgentEvent::Catalog(CatalogEvent::Models { current, .. }) => {
                if let Some(fence) = correlation.pings.front_mut() {
                    fence.model = current.clone();
                }
            }
            AgentEvent::Interaction(InteractionEvent::Error { code, message })
                if code == "model-failed" =>
            {
                if let Some(fence) = correlation.pings.front_mut() {
                    fence.error = Some(message.clone());
                }
            }
            AgentEvent::Interaction(InteractionEvent::Heartbeat) => {
                if let Some(fence) = correlation.pings.pop_front() {
                    if let (Some(record_id), Some(session_id)) = (fence.record_id, fence.session_id)
                    {
                        let result = fence.error.map_or_else(
                            || fence.model.ok_or_else(|| "No authoritative model at the dispatch fence; update the DSH bridge".into()),
                            Err,
                        );
                        return AgentEvent::Interaction(InteractionEvent::PromptStateCaptured {
                            record_id,
                            session_id,
                            result,
                        });
                    }
                }
            }
            _ => {}
        }
        event
    }

    async fn send_ping(&self, record_id: Option<u64>) -> Result<(), String> {
        {
            let mut correlation = self.correlation.lock().unwrap();
            if correlation.pings.len() >= 64 {
                return Err("Too many outstanding DSH state fences".into());
            }
            let session_id = correlation.session_id.clone();
            correlation.pings.push_back(Fence {
                record_id,
                session_id,
                ..Fence::default()
            });
        }
        self.outbound
            .send(ClientMessage::Ping)
            .await
            .map_err(|_| "bridge outbound channel closed".into())
    }
}

impl BridgeTransportPort for PromptStateTransport {
    async fn send_message(&self, message: ClientMessage) -> Result<(), String> {
        if matches!(message, ClientMessage::Ping) {
            self.send_ping(None).await
        } else {
            self.outbound
                .send(message)
                .await
                .map_err(|_| "bridge outbound channel closed".into())
        }
    }

    async fn send_request(&self, request: AgentRequest) -> Result<(), String> {
        if let AgentRequest::CapturePromptState { record_id } = request {
            self.send_ping(Some(record_id)).await
        } else {
            self.send_message(super::adapter::agent_request_to_client(request)?)
                .await
        }
    }
}
