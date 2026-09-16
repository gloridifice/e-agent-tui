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
mod color;
pub mod command_catalog;
pub mod config;
pub mod copy;
pub mod display;
pub mod event;
pub mod execution_capture;
pub mod execution_history;
mod help;
pub mod history_page;
pub mod i18n;
i18n::init_i18n!();
pub mod input;
pub mod input_page;
pub mod interaction;
pub mod key_mapping;
pub mod link_copy;
pub mod login;
pub mod mermaid;
pub mod model_defaults;
pub mod model_marks;
pub mod mouse_selection;
pub mod notice;
pub mod page_core;
pub mod path_completion;
pub mod presentation;
pub mod preview;
pub mod profile;
pub mod projection;
pub mod question;
pub mod reading;
pub mod render;
pub mod render_state;
pub mod resume;
pub mod reveal;
pub mod runtime;
pub mod settings;
pub mod syntax;
pub mod theme;
pub mod transcript_layout;
pub mod ui;
mod wrap;

pub use action::{
    clipboard_preview, AgentRequest, ClipboardPaste, DirtyState, DrawPriority, EffectResult,
    PromptImage, PromptInput, PromptPart, UiAction, UpdateResult,
};
pub use agent::AgentEvent;
pub use app::{FrontendKind, NewConversationDraft, SessionModel, SessionStatus, TuiApp};
pub use catalog::CatalogModel;
pub use config::{Config, HexRgb, PaneWidthPercent, RevealRate, ThinkingDisplayMode};
pub use event::{InputEvent, PointerEvent};
pub use i18n::Language;
pub use interaction::{
    HelpScrollState, InteractionModel, PaneResizeDrag, PaneResizeState, ScrollState,
    MIN_PREVIEW_COLUMNS, MIN_PREVIEW_PANE_WIDTH, PREVIEW_RIGHT_MARGIN_COLUMNS,
    PREVIEW_SEPARATOR_COLUMNS, PREVIEW_SEPARATOR_GAP_COLUMNS,
};
pub use mouse_selection::{MouseSelection, SelectionFrame, SelectionUpdate};
pub use notice::{NoticeState, COPY_NOTICE_MIN_SECS};
pub use preview::{
    LineSelection, MutationDiff, MutationHunk, PreviewCache, PreviewContent, PreviewKey,
    PreviewPaneState, PreviewPolicy, PreviewRef, PreviewRequest, PreviewRequestId,
    PreviewRevealIntent, PreviewRevision, PreviewState, PreviewTarget, PreviewWorkStats,
    ToolMetrics, ToolPreview, ToolPreviewPrimary, ToolPreviewSecondary,
};
pub use projection::TimelineModel;
pub use reading::{
    BlockId, ItemId, ReadingBlock, ReadingBlockKind, ReadingCopyPayload, ReadingDirection,
    ReadingDocument, ReadingItem, ReadingItemFragment, ReadingItemKind, ReadingLayout,
    ReadingViewState,
};
pub use render_state::{ActivityTransition, RenderState};
pub use theme::{Theme, ThemeFile, ThemeStyle};
