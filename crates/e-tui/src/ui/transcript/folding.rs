use super::*;

const ACTIVITY_FOLD_EDGE_ROWS: usize = 3;
const ACTIVITY_FOLD_THRESHOLD: usize = ACTIVITY_FOLD_EDGE_ROWS * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodePresentation {
    Hidden,
    Visible,
    ActivityFold { hidden: usize },
}

pub(super) fn is_collapsible_activity(item: &DisplayItem, state: &TuiApp) -> bool {
    match item {
        DisplayItem::Activity(_) => true,
        DisplayItem::Thinking(node) => {
            !state.config.thinking_display_mode().shows_reasoning() || node.content.is_empty()
        }
        _ => false,
    }
}

pub(super) fn transcript_presentations(state: &TuiApp) -> Vec<NodePresentation> {
    transcript_presentations_with_folding(state, state.reading.is_none())
}

pub(super) fn is_activity_fold_trigger(item: &DisplayItem) -> bool {
    let DisplayItem::Block(block) = item else {
        return false;
    };
    block.format == TranscriptFormat::Markdown
        || block.tone == DisplayTone::Error
        || block.id.0.starts_with("turn-outcome:")
        || block.id.0.ends_with(":turn-outcome")
}

pub(super) fn fold_activity_run(run: &[usize], presentations: &mut [NodePresentation]) {
    if run.len() <= ACTIVITY_FOLD_THRESHOLD {
        return;
    }
    let hidden = run.len() - ACTIVITY_FOLD_THRESHOLD;
    let middle = &run[ACTIVITY_FOLD_EDGE_ROWS..run.len() - ACTIVITY_FOLD_EDGE_ROWS];
    presentations[middle[0]] = NodePresentation::ActivityFold { hidden };
    for index in &middle[1..] {
        presentations[*index] = NodePresentation::Hidden;
    }
}

pub(super) fn transcript_presentations_with_folding(
    state: &TuiApp,
    fold_activities: bool,
) -> Vec<NodePresentation> {
    let nodes = state.transcript.nodes();
    let mut presentations = nodes
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if is_hidden_node(nodes, index, state) {
                NodePresentation::Hidden
            } else {
                NodePresentation::Visible
            }
        })
        .collect::<Vec<_>>();
    if !fold_activities {
        return presentations;
    }

    let mut run = Vec::new();
    let mut pending_runs = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if is_hidden_node(nodes, index, state) {
            continue;
        }
        if is_collapsible_activity(&node.item, state) {
            run.push(index);
            continue;
        }
        if !run.is_empty() {
            pending_runs.push(std::mem::take(&mut run));
        }
        if is_activity_fold_trigger(&node.item) {
            for pending in pending_runs.drain(..) {
                fold_activity_run(&pending, &mut presentations);
            }
        } else if !is_activity_item(&node.item) {
            pending_runs.clear();
        }
    }
    presentations
}

pub(super) fn folded_activity_line(hidden: usize, state: &TuiApp) -> Line<'static> {
    let noun = crate::i18n::tr(
        state.config.language,
        if hidden == 1 {
            "transcript.line"
        } else {
            "transcript.lines"
        },
    );
    Line::styled(
        crate::i18n::tr_args(
            state.config.language,
            "transcript.activity_fold",
            &[("count", hidden.to_string()), ("noun", noun)],
        ),
        state.theme().activity.detail.style(),
    )
}
