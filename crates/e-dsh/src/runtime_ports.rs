//! Narrow synchronous ports used by the runtime effect executor.
//!
//! Bridge and terminal input are already adapted to typed `RuntimeInput` by
//! the Tokio composition root. These ports cover the synchronous capabilities
//! that otherwise tend to leak filesystem, clipboard, and wall-clock access
//! into business handlers.

use std::{future::Future, time::Instant};

use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::{
    config::{self, Config, StateFile},
    protocol::ClientMessage,
    terminal_runtime::TerminalOwner,
    theme::{self, ThemeFile},
};

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

pub trait TerminalEventPort {
    fn next_event(&mut self) -> impl Future<Output = Option<Result<Event, String>>> + Send;
}

pub struct ProductionTerminalEvents {
    stream: EventStream,
}

impl ProductionTerminalEvents {
    pub fn new() -> Self {
        Self {
            stream: EventStream::new(),
        }
    }
}

impl Default for ProductionTerminalEvents {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalEventPort for ProductionTerminalEvents {
    fn next_event(&mut self) -> impl Future<Output = Option<Result<Event, String>>> + Send {
        async move {
            self.stream
                .next()
                .await
                .map(|result| result.map_err(|error| error.to_string()))
        }
    }
}

pub trait TerminalLifecyclePort {
    fn restore_terminal(&mut self) -> Result<(), String>;
}

impl TerminalLifecyclePort for TerminalOwner {
    fn restore_terminal(&mut self) -> Result<(), String> {
        self.restore().map_err(|error| error.to_string())
    }
}

pub trait UiActionPorts {
    fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String>;
    fn persist_config(&mut self, config: &Config) -> Result<(), String>;
    fn persist_session_id(&mut self, session_id: String);
    fn write_clipboard(&mut self, text: String) -> Result<(), String>;
    fn now(&self) -> Instant;
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

    fn write_clipboard(&mut self, text: String) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(text))
            .map_err(|error| error.to_string())
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
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
pub struct ScriptedTerminalEvents {
    events: std::collections::VecDeque<Result<Event, String>>,
}

#[cfg(test)]
impl ScriptedTerminalEvents {
    pub fn new(events: impl IntoIterator<Item = Result<Event, String>>) -> Self {
        Self {
            events: events.into_iter().collect(),
        }
    }
}

#[cfg(test)]
impl TerminalEventPort for ScriptedTerminalEvents {
    fn next_event(&mut self) -> impl Future<Output = Option<Result<Event, String>>> + Send {
        std::future::ready(self.events.pop_front())
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct ScriptedTerminalLifecycle {
    pub restore_calls: usize,
    pub fail: bool,
}

#[cfg(test)]
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
pub struct ScriptedRuntimePorts {
    pub loaded_config: Option<(Config, Vec<ThemeFile>)>,
    pub persisted_configs: usize,
    pub session_ids: Vec<String>,
    pub clipboard_writes: Vec<String>,
    pub config_result: Result<(), String>,
    pub clipboard_result: Result<(), String>,
    pub now: Instant,
}

#[cfg(test)]
impl ScriptedRuntimePorts {
    pub fn successful(now: Instant) -> Self {
        Self {
            loaded_config: None,
            persisted_configs: 0,
            session_ids: Vec::new(),
            clipboard_writes: Vec::new(),
            config_result: Ok(()),
            clipboard_result: Ok(()),
            now,
        }
    }
}

#[cfg(test)]
impl UiActionPorts for ScriptedRuntimePorts {
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

    fn write_clipboard(&mut self, text: String) -> Result<(), String> {
        self.clipboard_writes.push(text);
        self.clipboard_result.clone()
    }

    fn now(&self) -> Instant {
        self.now
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
