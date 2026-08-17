//! Materialization of Markdown/source-map presentation data from the pure
//! session message projection. This module owns renderer calls and unit-id
//! reuse; model event reduction only stores assistant source text.

use crate::{
    model::{AppState, Msg},
    render::{render_markdown, RenderOptions},
};

fn options(state: &AppState) -> RenderOptions {
    RenderOptions {
        expanded: state.expanded.clone(),
        collapse_rows: state.config.atomic_collapse_rows,
        mermaid_enabled: state.config.mermaid_enabled,
    }
}

pub fn materialize_assistants(state: &mut AppState) {
    let render_options = options(state);
    let theme = state.config.theme();
    let mut next_unit = state.next_unit;
    for message in &mut state.msgs {
        if let Msg::Assistant {
            text,
            lines,
            unit_start,
        } = message
        {
            if !text.is_empty() && lines.is_empty() {
                *unit_start = next_unit;
                *lines = render_markdown(
                    text,
                    &theme,
                    &mut next_unit,
                    &render_options,
                    &mut state.units,
                );
            }
        }
    }
    state.next_unit = state.next_unit.max(next_unit);
}

pub fn toggle_expand(state: &mut AppState, unit: u64) {
    if !state.units.contains_key(&unit) {
        return;
    }
    if !state.expanded.remove(&unit) {
        state.expanded.insert(unit);
    }
    let render_options = options(state);
    let theme = state.config.theme();
    for message in &mut state.msgs {
        if let Msg::Assistant {
            text,
            lines,
            unit_start,
        } = message
        {
            if lines.iter().any(|line| line.unit == unit) {
                let mut next_unit = *unit_start;
                *lines = render_markdown(
                    text,
                    &theme,
                    &mut next_unit,
                    &render_options,
                    &mut state.units,
                );
                state.next_unit = state.next_unit.max(next_unit);
                state.transcript_cache.invalidate();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_source_is_materialized_outside_the_event_reducer() {
        let mut state = AppState::default();
        state.msgs.push(Msg::Assistant {
            text: "# Title\n\nbody".into(),
            lines: Vec::new(),
            unit_start: 0,
        });
        materialize_assistants(&mut state);
        let Msg::Assistant { lines, .. } = &state.msgs[0] else {
            panic!("assistant")
        };
        assert!(!lines.is_empty());
        assert!(!state.units.is_empty());
    }
}
