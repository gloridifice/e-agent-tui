//! Narrow synchronous ports used by the runtime effect executor.
//!
//! Bridge and terminal input are already adapted to typed `RuntimeInput` by
//! the Tokio composition root. These ports cover the synchronous capabilities
//! that otherwise tend to leak filesystem, clipboard, and wall-clock access
//! into business handlers.

use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use tokio::sync::mpsc;

use crate::{
    config::{self, Config, StateFile},
    protocol::ClientMessage,
    theme::{self, ThemeFile},
};
pub use e_tui::runtime::{TerminalEventPort, TerminalLifecyclePort, UiActionPorts};
use e_tui::{ClipboardPaste, PreviewContent, PreviewRequest, PromptImage};

static CLIPBOARD_IMAGE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub trait BridgeTransportPort {
    fn send_message(
        &self,
        message: ClientMessage,
    ) -> impl Future<Output = Result<(), String>> + Send;
}

impl BridgeTransportPort for mpsc::Sender<ClientMessage> {
    fn send_message(
        &self,
        message: ClientMessage,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async move {
            self.send(message)
                .await
                .map_err(|_| "bridge outbound channel closed".to_owned())
        }
    }
}

#[derive(Default)]
pub struct ProductionRuntimePorts;

impl UiActionPorts for ProductionRuntimePorts {
    fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String> {
        let mut config = config::load();
        let themes = theme::load_themes(&config::themes_dir());
        config.resolved_theme = theme::resolve(&config.theme, &themes);
        Ok((config, themes))
    }

    fn persist_config(&mut self, config: &Config) -> Result<(), String> {
        config::save(config)
    }

    fn persist_session_id(&mut self, session_id: String) {
        let mut state = StateFile::load();
        state.last_session_id = Some(session_id);
        state.save();
    }

    fn read_clipboard(&mut self) -> Result<ClipboardPaste, String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        if let Ok(image) = clipboard.get_image() {
            let data = encode_clipboard_png(image.width, image.height, image.bytes.as_ref())?;
            let sequence = CLIPBOARD_IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            return Ok(ClipboardPaste::Image(PromptImage {
                media_type: "image/png".into(),
                data,
                name: Some(format!("clipboard-{}-{sequence}.png", std::process::id())),
            }));
        }
        clipboard
            .get_text()
            .map(ClipboardPaste::Text)
            .map_err(|error| error.to_string())
    }

    fn write_clipboard(&mut self, text: String) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(text))
            .map_err(|error| error.to_string())
    }

    fn resolve_preview(
        &mut self,
        request: PreviewRequest,
    ) -> impl Future<Output = Result<PreviewContent, String>> + Send {
        crate::preview_resolver::resolve(request)
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

fn encode_clipboard_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "clipboard image dimensions overflow".to_owned())?;
    if width == 0 || height == 0 || rgba.len() != expected {
        return Err("clipboard image has invalid RGBA dimensions".into());
    }
    let width = u32::try_from(width).map_err(|_| "clipboard image width is too large")?;
    let height = u32::try_from(height).map_err(|_| "clipboard image height is too large")?;
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("encode clipboard image: {error}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| format!("encode clipboard image: {error}"))?;
    }
    Ok(encoded)
}

#[cfg(test)]
pub struct ScriptedBridgeTransport {
    pub sent: std::sync::Mutex<Vec<ClientMessage>>,
    pub fail: bool,
}

#[cfg(test)]
impl BridgeTransportPort for ScriptedBridgeTransport {
    fn send_message(
        &self,
        message: ClientMessage,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async move {
            if self.fail {
                Err("scripted transport closed".into())
            } else {
                self.sent.lock().unwrap().push(message);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
use e_tui::runtime::ports::{
    ScriptedTerminalEvents, ScriptedTerminalLifecycle,
    ScriptedUiActionPorts as ScriptedRuntimePorts,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn clipboard_rgba_encodes_as_png() {
        let encoded = encode_clipboard_png(1, 1, &[0, 255, 0, 255]).unwrap();
        assert_eq!(&encoded[..8], b"\x89PNG\r\n\x1a\n");
        assert!(encode_clipboard_png(1, 1, &[0; 3]).is_err());
    }

    #[tokio::test]
    async fn scripted_terminal_and_transport_are_deterministic() {
        let event = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let mut terminal = ScriptedTerminalEvents::new([Ok(event.clone()), Err("closed".into())]);
        assert!(matches!(terminal.next_event().await, Some(Ok(next)) if next == event));
        assert!(matches!(terminal.next_event().await, Some(Err(error)) if error == "closed"));
        assert!(terminal.next_event().await.is_none());

        let transport = ScriptedBridgeTransport {
            sent: std::sync::Mutex::new(Vec::new()),
            fail: false,
        };
        transport
            .send_message(ClientMessage::Interrupt)
            .await
            .unwrap();
        assert!(matches!(
            transport.sent.lock().unwrap().as_slice(),
            [ClientMessage::Interrupt]
        ));

        let mut ports = ScriptedRuntimePorts::successful(Instant::now());
        ports
            .preview_results
            .push_back(Ok(PreviewContent::PlainText("resolved".into())));
        let request = e_tui::PreviewRequest {
            request_id: e_tui::PreviewRequestId(1),
            key: e_tui::PreviewKey("file:test".into()),
            revision: e_tui::PreviewRevision(2),
        };
        assert!(matches!(
            ports.resolve_preview(request.clone()).await,
            Ok(PreviewContent::PlainText(text)) if text == "resolved"
        ));
        assert_eq!(ports.preview_requests, vec![request]);
    }

    #[test]
    fn scripted_terminal_restore_is_observable() {
        let mut terminal = ScriptedTerminalLifecycle::default();
        terminal.restore_terminal().unwrap();
        terminal.restore_terminal().unwrap();
        assert_eq!(terminal.restore_calls, 2);
        terminal.fail = true;
        assert!(terminal.restore_terminal().is_err());
    }

    #[test]
    fn scripted_effect_ports_record_payloads_and_failures() {
        let now = Instant::now();
        let mut ports = ScriptedRuntimePorts::successful(now);
        ports.persist_session_id("s1".into());
        assert_eq!(ports.session_ids, ["s1"]);
        assert_eq!(ports.now(), now);
        ports.clipboard_result = Err("denied".into());
        assert_eq!(ports.write_clipboard("copy".into()), Err("denied".into()));
        assert_eq!(ports.clipboard_writes, ["copy"]);
    }
}
