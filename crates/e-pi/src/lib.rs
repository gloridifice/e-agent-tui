#![forbid(unsafe_code)]

//! Pi RPC adapter and `pie` executable infrastructure.
//!
//! Pi owns agent semantics and persistent state. This package owns only the
//! child-process transport and conversion to/from `e-tui`'s normalized API.

pub mod adapter;
pub mod config;
pub mod framing;
pub mod process;
pub mod protocol;
pub mod session_index;
