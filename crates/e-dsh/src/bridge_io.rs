//! WebSocket tasks and bounded channels for the TUI runtime.

use anyhow::{anyhow, Context};
use futures_util::{SinkExt, StreamExt};
use std::io::ErrorKind;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async_with_config, tungstenite::Message};

use crate::{
    dsh_env::PROFILE_NAME,
    protocol::{ClientMessage, ServerMessage, MAX_WIRE_FRAME_BYTES},
};

pub struct BridgeIo {
    pub outbound: mpsc::Sender<ClientMessage>,
    pub inbound: mpsc::Receiver<ServerMessage>,
    writer: JoinHandle<()>,
    reader: JoinHandle<()>,
}

fn is_transient_connect_error(error: &tokio_tungstenite::tungstenite::Error) -> bool {
    matches!(
        error,
        tokio_tungstenite::tungstenite::Error::Io(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionRefused
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::NotConnected
            )
    )
}

/// Build the actionable message for a failed bridge connection.
fn connection_error(url: &str, error: impl std::fmt::Display) -> anyhow::Error {
    anyhow!(
        "cannot connect to the DSH bridge at {url}: {error}. The DSH service stopped or the `dsh-tui` route is unavailable. Run `dsh --profile {PROFILE_NAME}` to inspect startup output; if DSH runs, remount with `tools\\mount-bridge.ps1 -Profile {PROFILE_NAME}`, run `dsh plugin --profile {PROFILE_NAME} install`, and restart DSH"
    )
}

impl BridgeIo {
    pub fn shutdown(&self) {
        self.writer.abort();
        self.reader.abort();
    }

    pub async fn connect(
        url: &str,
        hello: ClientMessage,
        max_frame_bytes: usize,
    ) -> anyhow::Result<Self> {
        let wire_limit = max_frame_bytes.max(MAX_WIRE_FRAME_BYTES);
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(wire_limit))
            .max_frame_size(Some(wire_limit));
        // A process can disappear in the small gap between the launcher's TCP
        // readiness probe and the WebSocket upgrade. Retry only transient I/O
        // failures; handshake/auth/protocol failures must surface immediately.
        let deadline = Instant::now() + Duration::from_secs(2);
        let ws = loop {
            match connect_async_with_config(url, Some(config.clone()), false).await {
                Ok((ws, _)) => break ws,
                Err(error) if is_transient_connect_error(&error) && Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
                Err(error) => return Err(connection_error(url, error)),
            }
        };
        let (sink, mut stream) = ws.split();
        let (inbound_tx, inbound) = mpsc::channel::<ServerMessage>(512);
        let (outbound, mut outbound_rx) = mpsc::channel::<ClientMessage>(128);
        let writer_events = inbound_tx.clone();
        let writer = tokio::spawn(async move {
            let mut sink = sink;
            while let Some(message) = outbound_rx.recv().await {
                let has_images = message.has_images();
                let Ok(wire) = message.to_wire() else {
                    continue;
                };
                if has_images && wire.len() > MAX_WIRE_FRAME_BYTES {
                    let _ = writer_events
                        .send(ServerMessage::Error {
                            code: "image-input-too-large".into(),
                            message: format!(
                                "encoded image prompt exceeds the {} byte wire limit",
                                MAX_WIRE_FRAME_BYTES
                            ),
                        })
                        .await;
                    continue;
                }
                if sink.send(Message::Text(wire.into())).await.is_err() {
                    break;
                }
            }
        });
        let reader = tokio::spawn(async move {
            while let Some(item) = stream.next().await {
                let item = match item {
                    Ok(item) => item,
                    Err(error) => {
                        eprintln!("[dshe] websocket stream error: {error}");
                        break;
                    }
                };
                match item {
                    Message::Text(text) => {
                        if let Some(message) = ServerMessage::from_wire(&text) {
                            if inbound_tx.send(message).await.is_err() {
                                break;
                            }
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            let _ = inbound_tx
                .send(ServerMessage::Error {
                    code: "disconnected".into(),
                    message: "bridge connection closed".into(),
                })
                .await;
        });
        outbound.send(hello).await.context("send hello")?;
        Ok(Self {
            outbound,
            inbound,
            writer,
            reader,
        })
    }
}

impl Drop for BridgeIo {
    fn drop(&mut self) {
        self.writer.abort();
        self.reader.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_connection_has_actionable_dsh_guidance() {
        let error = connection_error(
            "ws://127.0.0.1:1/dsh-tui",
            tokio_tungstenite::tungstenite::Error::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "connection refused",
            )),
        );
        let message = error.to_string();
        assert!(message.contains("cannot connect to the DSH bridge"));
        assert!(message.contains("dsh --profile e"));
        assert!(message.contains("mount-bridge.ps1"));
    }
}
