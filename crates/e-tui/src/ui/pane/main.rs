use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    Frame,
};

use crate::{
    app::TuiApp,
    display::InputAccessoryKind,
    input::InputState,
    input_page::InputPageSession,
    interaction::{ApprovalCard, PendingPrompt, ScrollState},
    login::LoginState,
    settings::SettingsState,
    theme::Theme,
};

use super::super::{accessories, layout, pages, region};

pub(crate) struct MainPaneOverlays<'a> {
    pub help_visible: bool,
    pub input_page: Option<&'a mut InputPageSession>,
    pub settings: Option<&'a mut SettingsState>,
    pub login: Option<&'a mut LoginState>,
    pub approval: Option<&'a ApprovalCard>,
    pub queue: &'a [PendingPrompt],
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_with_cursor(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: MainPaneOverlays<'_>,
    reserve_collapsed_separator: bool,
) -> Option<Position> {
    render_main_pane_with_cursor(
        frame,
        area,
        state,
        input,
        scroll,
        theme,
        overlays,
        reserve_collapsed_separator,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_main_pane_with_cursor(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiApp,
    input: &InputState,
    scroll: &mut ScrollState,
    theme: &Theme,
    overlays: MainPaneOverlays<'_>,
    reserve_collapsed_separator: bool,
) -> Option<Position> {
    let MainPaneOverlays {
        help_visible,
        mut input_page,
        mut settings,
        mut login,
        approval,
        queue,
    } = overlays;
    let page = layout::main_page_rect(area, state, reserve_collapsed_separator);
    let input_page_rows = input_page
        .as_deref()
        .map(pages::preferred_rows)
        .or_else(|| (settings.is_some() || login.is_some()).then_some(usize::MAX));
    let input_page_open = input_page_rows.is_some();
    let plan = layout::BottomLayoutPlan::new(
        area.height,
        page.width,
        state,
        input,
        input_page_rows,
        approval,
        queue,
        state.session.new_conversation.is_some(),
    );
    let bottom = Constraint::Length(plan.bottom_rows);
    let approval_rows = plan.rows(InputAccessoryKind::Approval);
    let goal_rows = plan.rows(InputAccessoryKind::Goal);
    let plan_rows = plan.rows(InputAccessoryKind::Plan);
    let todo_rows = plan.rows(InputAccessoryKind::Todo);
    let queue_visible = usize::from(plan.rows(InputAccessoryKind::Queue));

    if input_page_open {
        let chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(approval_rows),
            Constraint::Length(goal_rows),
            Constraint::Length(plan_rows),
            Constraint::Length(todo_rows),
            Constraint::Length(queue_visible as u16),
            bottom,
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(page);
        region::transcript::render(frame, chunks[0], state, scroll, theme, help_visible);
        if approval_rows > 0 {
            if let Some(card) = approval {
                accessories::render_approval(frame, chunks[1], card, theme, &state.config);
            }
        }
        if goal_rows > 0 {
            accessories::render_info_accessory(
                frame,
                chunks[2],
                "accessory.goal",
                state.goal.as_deref().unwrap_or(""),
                theme,
                state.config.language,
            );
        }
        if plan_rows > 0 {
            accessories::render_info_accessory(
                frame,
                chunks[3],
                "accessory.plan",
                state.plan_mode.as_deref().unwrap_or(""),
                theme,
                state.config.language,
            );
        }
        if todo_rows > 0 {
            accessories::render_todo(frame, chunks[4], &state.todos, theme, state.config.language);
        }
        if queue_visible > 0 {
            accessories::render_queue(
                frame,
                chunks[5],
                queue,
                queue_visible,
                theme,
                state.config.language,
            );
        }
        let cursor_anchor = if let Some(page) = input_page.as_mut() {
            region::input_page::render(frame, chunks[6], page, &state.config, theme)
        } else if let Some(settings) = settings.as_mut() {
            pages::render_settings(frame, chunks[6], settings, &state.config, theme);
            None
        } else if let Some(login) = login.as_mut() {
            pages::render_login(frame, chunks[6], login, theme, &state.config);
            None
        } else {
            region::composer::render(
                frame,
                chunks[6],
                input,
                theme,
                state.config.user_input_padding as u16,
                input.model_hint(&state.config, &state.catalogs),
            )
        };
        region::status::render(frame, chunks[8], state, scroll, theme);
        region::status::render_title(frame, chunks[9], state, theme);
        if !input_page_open {
            if let Some(suggest) = input.suggest.as_ref() {
                accessories::render_suggest(frame, suggest, chunks[6], theme);
            }
        }
        return cursor_anchor;
    }

    let bottom_stack = plan.bottom_stack;
    let transcript_bottom = region::transcript::render_combined(
        frame,
        page,
        state,
        scroll,
        theme,
        help_visible,
        bottom_stack,
    );
    let mut cursor_anchor = None;
    let mut input_rect = None;
    let mut y = page.y + transcript_bottom as u16;
    let end_y = page.y + page.height;

    if approval_rows > 0 && y < end_y {
        let h = (end_y - y).min(approval_rows);
        if let Some(card) = approval {
            accessories::render_approval(
                frame,
                Rect::new(page.x, y, page.width, h),
                card,
                theme,
                &state.config,
            );
        }
        y = y.saturating_add(approval_rows);
    }
    if goal_rows > 0 && y < end_y {
        let h = (end_y - y).min(goal_rows);
        accessories::render_info_accessory(
            frame,
            Rect::new(page.x, y, page.width, h),
            "accessory.goal",
            state.goal.as_deref().unwrap_or(""),
            theme,
            state.config.language,
        );
        y = y.saturating_add(goal_rows);
    }
    if plan_rows > 0 && y < end_y {
        let h = (end_y - y).min(plan_rows);
        accessories::render_info_accessory(
            frame,
            Rect::new(page.x, y, page.width, h),
            "accessory.plan",
            state.plan_mode.as_deref().unwrap_or(""),
            theme,
            state.config.language,
        );
        y = y.saturating_add(plan_rows);
    }
    if todo_rows > 0 && y < end_y {
        let h = (end_y - y).min(todo_rows);
        accessories::render_todo(
            frame,
            Rect::new(page.x, y, page.width, h),
            &state.todos,
            theme,
            state.config.language,
        );
        y = y.saturating_add(todo_rows);
    }
    if queue_visible > 0 && y < end_y {
        let h = (end_y - y).min(queue_visible as u16);
        accessories::render_queue(
            frame,
            Rect::new(page.x, y, page.width, h),
            queue,
            queue_visible,
            theme,
            state.config.language,
        );
        y = y.saturating_add(queue_visible as u16);
    }
    if y < end_y {
        let h = (end_y - y).min(plan.bottom_rows);
        let rect = Rect::new(page.x, y, page.width, h);
        input_rect = Some(rect);
        cursor_anchor = region::composer::render(
            frame,
            rect,
            input,
            theme,
            state.config.user_input_padding as u16,
            input.model_hint(&state.config, &state.catalogs),
        );
        y = y.saturating_add(plan.bottom_rows);
    }
    if y < end_y {
        if state.link_copy.armed {
            region::composer::render_link_copy_hint(
                frame,
                Rect::new(page.x, y, page.width, 1),
                theme,
                state.config.language,
            );
        }
        y = y.saturating_add(1);
    }
    if y < end_y {
        region::status::render(
            frame,
            Rect::new(page.x, y, page.width, 1),
            state,
            scroll,
            theme,
        );
        y = y.saturating_add(1);
    }
    if y < end_y {
        region::status::render_title(frame, Rect::new(page.x, y, page.width, 1), state, theme);
    }
    if let Some(suggest) = input.suggest.as_ref() {
        if let Some(rect) = input_rect {
            accessories::render_suggest(frame, suggest, rect, theme);
        }
    }
    cursor_anchor
}
