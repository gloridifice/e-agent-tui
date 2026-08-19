#![forbid(unsafe_code)]

//! Kernel-neutral terminal frontend for interactive agent sessions.
//!
//! The initial workspace split intentionally keeps the existing UI in the
//! executable package. Subsequent migration phases move normalized contracts
//! and presentation modules behind this library boundary without changing the
//! visible client behavior.

pub mod action;
pub mod agent;
pub mod assets;
pub mod cache;
pub mod command_catalog;
pub mod config;
pub mod copy;
pub mod display;
pub mod event;
pub mod input;
pub mod input_page;
pub mod interaction;
pub mod login;
pub mod mermaid;
pub mod page_core;
pub mod profile;
pub mod projection;
pub mod question;
pub mod render;
pub mod settings;
pub mod theme;
pub mod transcript_layout;

pub use action::{AgentRequest, DirtyState, DrawPriority, EffectResult, UiAction, UpdateResult};
pub use agent::AgentEvent;
pub use config::{Config, ThinkingDisplayMode};
pub use event::InputEvent;
pub use projection::TimelineModel;
pub use theme::{Theme, ThemeFile, ThemeStyle};
