#![forbid(unsafe_code)]

//! Pi RPC adapter and `pie` executable infrastructure.
//!
//! Pi owns agent semantics and persistent state. This package owns only the
//! child-process transport and conversion to/from `e-tui`'s normalized API.

pub mod adapter;
pub mod auth;
mod compaction_store;
pub mod config;
pub mod effects;
pub mod execution_history_store;
pub mod framing;
pub mod herdr;
pub mod path_completion;
pub mod process;
pub mod protocol;
pub mod session_index;
