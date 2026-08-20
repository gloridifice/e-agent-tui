#![forbid(unsafe_code)]

//! Kernel-neutral terminal frontend for interactive agent sessions.
//!
//! The initial workspace split intentionally keeps the existing UI in the
//! executable package. Subsequent migration phases move normalized contracts
//! and presentation modules behind this library boundary without changing the
//! visible client behavior.

pub mod action;
pub mod agent;
pub mod app;
pub mod assets;
pub mod cache;
pub mod catalog;
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
pub mod presentation;
pub mod preview;
pub mod profile;
pub mod projection;
pub mod question;
pub mod reading;
pub mod render;
pub mod render_state;
pub mod settings;
pub mod theme;
pub mod transcript_layout;
pub mod ui;
mod wrap;

pub use action::{AgentRequest, DirtyState, DrawPriority, EffectResult, UiAction, UpdateResult};
pub use agent::AgentEvent;
pub use app::{NewConversationDraft, SessionModel, SessionStatus, TuiApp};
pub use catalog::CatalogModel;
pub use config::{Config, ThinkingDisplayMode};
pub use event::InputEvent;
pub use interaction::{InteractionModel, ScrollState};
pub use preview::{
    LineSelection, MutationHunk, PreviewCache, PreviewContent, PreviewKey, PreviewPaneState,
    PreviewPolicy, PreviewRef, PreviewRequest, PreviewRequestId, PreviewRevision, PreviewState,
    PreviewTarget, PreviewWorkStats, ToolMetrics, ToolPreview, ToolPreviewPrimary,
    ToolPreviewSecondary,
};
pub use projection::TimelineModel;
pub use reading::{
    BlockId, ItemId, ReadingBlock, ReadingBlockKind, ReadingCopyPayload, ReadingDirection,
    ReadingDocument, ReadingItem, ReadingItemFragment, ReadingItemKind, ReadingLayout,
    ReadingViewState,
};
pub use render_state::{ActivityTransition, RenderState};
pub use theme::{Theme, ThemeFile, ThemeStyle};
