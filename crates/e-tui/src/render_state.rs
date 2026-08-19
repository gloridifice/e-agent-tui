//! Width- and materialization-dependent frontend state.

use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use ratatui::style::Color;

use crate::{
    cache::TranscriptRenderCache, display::DisplayId, transcript_layout::MarkdownLayoutRegistry,
};

#[derive(Debug, Clone)]
pub struct ActivityTransition {
    pub done_since: Instant,
    pub from: Color,
}

/// Domain transcript text remains exclusively in `TimelineModel`; this owner
/// keeps caches, provenance indexes, transitions, and stable unit allocators.
pub struct RenderState {
    pub markdown_layout: MarkdownLayoutRegistry,
    pub activity_transitions: HashMap<DisplayId, ActivityTransition>,
    pub next_unit: u64,
    pub units: HashMap<u64, String>,
    pub expanded: HashSet<u64>,
    pub transcript_cache: TranscriptRenderCache,
    pub stream_frame: usize,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            markdown_layout: MarkdownLayoutRegistry::default(),
            activity_transitions: HashMap::new(),
            next_unit: 0,
            units: HashMap::new(),
            expanded: HashSet::new(),
            transcript_cache: TranscriptRenderCache {
                width: 80,
                ..TranscriptRenderCache::default()
            },
            stream_frame: 0,
        }
    }
}
