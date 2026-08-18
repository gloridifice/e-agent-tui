//! Markdown materialization and expansion over the public transcript store.
//!
//! Source-only `TranscriptBlock`s remain the stored representation. Styled
//! `RenderLine`s and complete unit provenance live in the layout registry.

use crate::{
    display::{DisplayItem, TranscriptFormat},
    model::AppState,
    render::RenderOptions,
};

fn options(state: &AppState) -> RenderOptions {
    RenderOptions {
        expanded: state.expanded.clone(),
        collapse_rows: state.config.atomic_collapse_rows,
        mermaid_enabled: state.config.mermaid_enabled,
    }
}

pub fn materialize_transcript(state: &mut AppState) {
    let blocks = state
        .transcript
        .nodes()
        .iter()
        .filter_map(|node| match &node.item {
            DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown => {
                Some((block.id.clone(), block.content.clone(), block.unit))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let theme = state.config.theme();
    let render_options = options(state);
    for (id, source, current_unit) in blocks {
        let primary = state
            .markdown_layout
            .materialize(
                &id,
                &source,
                &theme,
                &mut state.next_unit,
                &render_options,
                &mut state.units,
            )
            .first()
            .map(|line| line.unit);
        if primary.is_some() && primary != current_unit {
            if let Some(node) = state.transcript.get_mut(&id) {
                if let DisplayItem::Block(block) = &mut node.item {
                    block.unit = primary;
                }
            }
            state.transcript.touch(&id);
        }
    }
}

pub fn toggle_expand(state: &mut AppState, unit: u64) {
    if !state.units.contains_key(&unit) {
        return;
    }
    let Some(id) = state.markdown_layout.display_for_unit(unit).cloned() else {
        return;
    };
    if !state.expanded.remove(&unit) {
        state.expanded.insert(unit);
    }
    state.markdown_layout.invalidate(&id);
    state.transcript_cache.invalidate();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_source_materializes_in_layout_sidecar() {
        let mut state = AppState::default();
        state.apply_event(&serde_json::json!({
            "type":"assistant/message", "seq":1,
            "data":{"message":{"content":[{"type":"text","text":"# Title\n\nbody"}]}}
        }));
        materialize_transcript(&mut state);
        let id = state.transcript.nodes()[0].id();
        assert!(!state.markdown_layout.lines(id).unwrap().is_empty());
        assert!(!state.units.is_empty());
    }

    #[test]
    fn expansion_invalidates_the_owning_display_without_changing_identity() {
        let mut state = AppState::default();
        state.apply_event(&serde_json::json!({
            "type":"assistant/message", "seq":1,
            "data":{"message":{"content":[{"type":"text","text":"```\na\n```"}]}}
        }));
        materialize_transcript(&mut state);
        let id = state.transcript.nodes()[0].id().clone();
        let unit = state.markdown_layout.lines(&id).unwrap()[0].unit;
        state.transcript_cache.valid = true;
        toggle_expand(&mut state, unit);
        assert!(state.expanded.contains(&unit));
        assert_eq!(state.transcript.nodes()[0].id(), &id);
        assert!(!state.transcript_cache.valid);
    }
}
