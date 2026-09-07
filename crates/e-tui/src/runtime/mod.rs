//! Provider-neutral frontend runtime coordination.
//!
//! Executable adapters own their transports, process lifecycle, persistence,
//! clipboard, and deferred I/O implementations. This module owns only shared
//! terminal/frontend mechanics over normalized `e-tui` values.

pub mod command;
pub mod controller;
pub mod executor;
pub mod input;
pub mod policy;
pub mod ports;
pub mod scheduler;
pub mod state;
pub mod terminal;

pub use controller::{RuntimeController, RuntimeUiState, TerminalUiState};
pub use executor::{execute_ui_actions, EffectExecution};
pub use input::{
    route_terminal_event_with_mapping as route_terminal_event, ProductionTerminalEvents,
    TerminalFocus, TerminalRoute,
};
pub use policy::{
    animation_interval, inbound_budget_remaining, is_streaming_delta, wait_for_deadline,
    INBOUND_BATCH_BUDGET, INBOUND_BATCH_LIMIT, MIN_ANIMATION_INTERVAL,
};
pub use ports::{AgentRequestPort, TerminalEventPort, TerminalLifecyclePort, UiActionPorts};
#[cfg(any(test, feature = "test-support"))]
pub use ports::{
    ScriptedAgentRequestPort, ScriptedTerminalEvents, ScriptedTerminalLifecycle,
    ScriptedUiActionPorts,
};
pub use scheduler::{
    DirtyReason, FrameScheduler, CONTENT_FRAME_INTERVAL, INTERACTIVE_FRAME_INTERVAL,
};
pub use state::{animation_active, tick_spinners, RuntimeState};
pub use terminal::{FrameTransaction, TerminalOwner};
