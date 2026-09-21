use ratatui::layout::Rect;

use crate::{
    app::TuiApp,
    display::{allocate_accessories, AccessoryAllocation, InputAccessory, InputAccessoryKind},
    input::{layout::InputLayout, InputState},
    interaction::{
        ApprovalCard, PendingPrompt, PREVIEW_RIGHT_MARGIN_COLUMNS, PREVIEW_SEPARATOR_COLUMNS,
        PREVIEW_SEPARATOR_GAP_COLUMNS,
    },
};

/// Ordinary Main content keeps one blank column at each pane edge.
pub(crate) const MAIN_PAGE_MARGIN: u16 = 1;

fn main_page_insets(reserve_collapsed_separator: bool) -> (u16, u16) {
    let right = if reserve_collapsed_separator {
        PREVIEW_SEPARATOR_COLUMNS + PREVIEW_SEPARATOR_GAP_COLUMNS + PREVIEW_RIGHT_MARGIN_COLUMNS
    } else {
        MAIN_PAGE_MARGIN
    };
    (MAIN_PAGE_MARGIN, right)
}

fn content_page_width(main: Rect, max_width: u16, reserve_collapsed_separator: bool) -> u16 {
    let (left, right) = main_page_insets(reserve_collapsed_separator);
    let available = main.width.saturating_sub(left.saturating_add(right));
    if max_width > 0 {
        max_width.min(available)
    } else {
        available
    }
}

/// Content page rectangle shared by Screen, Main Pane, and runtime scroll
/// measurement. Keeping this policy below those callers prevents a rendering
/// child from calling back into the UI composition root.
pub(crate) fn main_page_rect(
    main: Rect,
    state: &TuiApp,
    reserve_collapsed_separator: bool,
) -> Rect {
    let (left, right) = main_page_insets(reserve_collapsed_separator);
    let max_width = state.config.page_max_width as u16;
    let width = content_page_width(main, max_width, reserve_collapsed_separator);
    let slack = main
        .width
        .saturating_sub(left.saturating_add(right).saturating_add(width));
    let offset = match state.config.page_align.as_str() {
        "left" => 0,
        "right" => slack,
        _ => slack / 2,
    };
    let x = main.x.saturating_add(left).saturating_add(offset);
    Rect::new(x, main.y, width, main.height)
}

pub(crate) const INPUT_MAX_ROWS: usize = 5;

pub(crate) fn input_rows(
    input: &InputState,
    wrap_width: usize,
    padding: usize,
    model_hint: Option<&str>,
) -> usize {
    let inner = wrap_width
        .saturating_sub(padding.saturating_mul(2).saturating_add(1))
        .max(1);
    let (display, _) = input.display_with_model_hint(model_hint);
    InputLayout::new(&display, inner)
        .chunks
        .len()
        .clamp(1, INPUT_MAX_ROWS)
}

pub(crate) fn bottom_area_rows(
    area_height: u16,
    area_width: u16,
    input: &InputState,
    input_page_rows: Option<usize>,
    padding: usize,
    model_hint: Option<&str>,
) -> u16 {
    if let Some(rows) = input_page_rows {
        rows.min(usize::from(area_height) * 2 / 3)
            .min(usize::from(area_height.saturating_sub(3))) as u16
    } else {
        (input_rows(input, area_width as usize, padding, model_hint) + 2) as u16
    }
}

fn input_accessories(
    state: &TuiApp,
    approval: Option<&ApprovalCard>,
    queue: &[PendingPrompt],
) -> Vec<InputAccessory> {
    let mut accessories = Vec::new();
    if approval.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Approval,
            priority: 90,
            desired_rows: 3,
            minimum_rows: 3,
            blocking: true,
            insertion_order: 0,
        });
    }
    if state.goal.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Goal,
            priority: 40,
            desired_rows: 1,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 1,
        });
    }
    if state.plan_mode.is_some() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Plan,
            priority: 30,
            desired_rows: 1,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 2,
        });
    }
    if !state.todos.is_empty() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Todo,
            priority: 20,
            desired_rows: (state.todos.len() + 1).min(6) as u16,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 3,
        });
    }
    if !queue.is_empty() {
        accessories.push(InputAccessory {
            kind: InputAccessoryKind::Queue,
            priority: 10,
            desired_rows: queue.len().min(u16::MAX as usize) as u16,
            minimum_rows: 1,
            blocking: false,
            insertion_order: 4,
        });
    }
    accessories
}

/// One measured bottom stack shared by drawing and runtime scroll/page math.
/// The ordinary and Input Page renderers still position their rectangles
/// differently; they consume the same row decisions.
pub(crate) struct BottomLayoutPlan {
    pub bottom_rows: u16,
    pub allocations: Vec<AccessoryAllocation>,
    pub bottom_stack: usize,
}

impl BottomLayoutPlan {
    pub(crate) fn new(
        area_height: u16,
        page_width: u16,
        state: &TuiApp,
        input: &InputState,
        input_page_rows: Option<usize>,
        approval: Option<&ApprovalCard>,
        queue: &[PendingPrompt],
        drafting: bool,
    ) -> Self {
        let bottom_rows = bottom_area_rows(
            area_height,
            page_width,
            input,
            input_page_rows,
            state.config.user_input_padding,
            input.model_hint(&state.config, &state.catalogs),
        );
        let accessories = if drafting {
            Vec::new()
        } else {
            input_accessories(state, approval, queue)
        };
        let budget = area_height.saturating_sub(1 + bottom_rows + 3);
        let allocations = allocate_accessories(&accessories, budget);
        let bottom_stack = allocations
            .iter()
            .map(|allocation| usize::from(allocation.rows))
            .sum::<usize>()
            + usize::from(bottom_rows)
            + 3;
        Self {
            bottom_rows,
            allocations,
            bottom_stack,
        }
    }

    pub(crate) fn rows(&self, kind: InputAccessoryKind) -> u16 {
        self.allocations
            .iter()
            .find(|allocation| allocation.kind == kind)
            .map_or(0, |allocation| allocation.rows)
    }
}
