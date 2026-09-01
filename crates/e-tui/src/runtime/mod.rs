//! Provider-neutral frontend runtime coordination.
//!
//! Executable adapters own their transports, process lifecycle, persistence,
//! clipboard, and deferred I/O implementations. This module owns only shared
//! terminal/frontend mechanics over normalized `e-tui` values.

pub mod command;
pub mod controller;
pub mod input;
pub mod ports;
pub mod scheduler;
pub mod state;
pub mod terminal;

pub use controller::{RuntimeController, RuntimeUiState, TerminalUiState};
pub use input::{route_terminal_event, ProductionTerminalEvents, TerminalFocus, TerminalRoute};
#[cfg(any(test, feature = "test-support"))]
pub use ports::{ScriptedTerminalEvents, ScriptedTerminalLifecycle, ScriptedUiActionPorts};
pub use ports::{TerminalEventPort, TerminalLifecyclePort, UiActionPorts};
pub use scheduler::{
    DirtyReason, FrameScheduler, CONTENT_FRAME_INTERVAL, INTERACTIVE_FRAME_INTERVAL,
};
pub use state::{animation_active, tick_spinners, RuntimeState};
pub use terminal::{FrameTransaction, TerminalOwner};
