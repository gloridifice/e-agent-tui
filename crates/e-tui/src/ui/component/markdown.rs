use crate::{app::TuiApp, display::DisplayId, render::RenderLine};

pub fn materialized_lines<'a>(state: &'a TuiApp, id: &DisplayId) -> Option<&'a [RenderLine]> {
    state.render.markdown_layout.lines(id)
}
