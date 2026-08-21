//! DSH adapter, runtime composition, setup, and executable infrastructure.

pub mod bridge;
pub mod bridge_io;
pub mod config;
pub mod dsh_env;
pub mod launcher;
pub mod model;
pub mod preview_resolver;
pub mod profile;
pub mod protocol;
pub mod runtime;
pub mod runtime_command;
pub mod runtime_ports;
pub mod setup;
pub mod terminal_runtime;
pub mod theme;
pub mod vt_input;
#[cfg(windows)]
pub mod win_input;
