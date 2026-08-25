//! Markdown materialization and expansion over the canonical transcript.

use crate::{
    app::TuiApp,
    display::{DisplayItem, TranscriptFormat},
    render::RenderOptions,
};

fn options(state: &TuiApp) -> RenderOptions {
    RenderOptions {
        expanded: state.render.expanded.clone(),
        collapse_rows: state.config.atomic_collapse_rows,
        mermaid_enabled: state.config.mermaid_enabled,
        markdown_strength: Default::default(),
        content_width: Some(state.render.transcript_cache.width),
    }
}

pub fn materialize_transcript(state: &mut TuiApp) {
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
        let primary = {
            let render = &mut state.render;
            render
                .markdown_layout
                .materialize(
                    &id,
                    &source,
                    &theme,
                    &mut render.next_unit,
                    &render_options,
                    &mut render.units,
                )
                .first()
                .map(|line| line.unit)
        };
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

pub fn toggle_expand(state: &mut TuiApp, unit: u64) {
    if !state.render.units.contains_key(&unit) {
        return;
    }
    let Some(id) = state.render.markdown_layout.display_for_unit(unit).cloned() else {
        return;
    };
    if !state.render.expanded.remove(&unit) {
        state.render.expanded.insert(unit);
    }
    state.render.markdown_layout.invalidate(&id);
    state.render.transcript_cache.invalidate();
}
