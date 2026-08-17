//! WebSocket tasks and bounded channels for the TUI runtime.

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async_with_config, tungstenite::Message};

use crate::protocol::{ClientMessage, ServerMessage, MAX_WIRE_FRAME_BYTES};

pub struct BridgeIo {
    pub outbound: mpsc::Sender<ClientMessage>,
    pub inbound: mpsc::Receiver<ServerMessage>,
    writer: JoinHandle<()>,
    reader: JoinHandle<()>,
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
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
            max_message_size: Some(max_frame_bytes.max(MAX_WIRE_FRAME_BYTES)),
            max_frame_size: Some(max_frame_bytes.max(MAX_WIRE_FRAME_BYTES)),
            ..Default::default()
        };
        let (ws, _) = connect_async_with_config(url, Some(config), false)
            .await
            .with_context(|| format!("connect {url}"))?;
        let (sink, mut stream) = ws.split();
        let (outbound, mut outbound_rx) = mpsc::channel::<ClientMessage>(128);
        let writer = tokio::spawn(async move {
            let mut sink = sink;
            while let Some(message) = outbound_rx.recv().await {
                let Ok(wire) = message.to_wire() else {
                    continue;
                };
                if sink.send(Message::Text(wire)).await.is_err() {
                    break;
                }
            }
        });
        let (inbound_tx, inbound) = mpsc::channel::<ServerMessage>(512);
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
