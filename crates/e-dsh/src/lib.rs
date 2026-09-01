//! DSH adapter, runtime composition, setup, and executable infrastructure.

pub mod bridge;
pub mod bridge_io;
pub mod config;
pub mod dsh_env;
pub mod launcher;
pub mod preview_resolver;
pub mod protocol;
pub mod runtime_ports;
pub mod setup;
pub mod theme;
#[cfg(windows)]
pub mod win_input;
