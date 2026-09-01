//! Narrow external-effect seams for executable adapters and scripted tests.

use std::{future::Future, time::Instant};

use crossterm::event::Event;

use crate::{Config, PreviewContent, PreviewRequest, ThemeFile};

pub trait TerminalEventPort {
    fn next_event(&mut self) -> impl Future<Output = Option<Result<Event, String>>> + Send;
}

pub trait TerminalLifecyclePort {
    fn restore_terminal(&mut self) -> Result<(), String>;
}

pub trait UiActionPorts {
    fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String>;
    fn persist_config(&mut self, config: &Config) -> Result<(), String>;
    fn persist_session_id(&mut self, session_id: String);
    fn read_clipboard(&mut self) -> Result<String, String>;
    fn write_clipboard(&mut self, text: String) -> Result<(), String>;
    fn resolve_preview(
        &mut self,
        request: PreviewRequest,
    ) -> impl Future<Output = Result<PreviewContent, String>> + Send;
    fn now(&self) -> Instant;
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
