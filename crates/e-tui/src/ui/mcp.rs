//! Screen-level MCP modal; rendering reads only safe in-memory metadata.

use crate::{
    key_mapping::{Action, Scope},
    mcp::{McpPatch, McpState, McpView},
    theme::Theme,
    Config,
};
use ratatui::{
    layout::{Alignment, Rect},
    style::Modifier,
    text::Line,
    widgets::{Block, Clear, Paragraph},
    Frame,
};

fn wrapped(text: &str, width: u16) -> Vec<Line<'static>> {
    text.lines()
        .flat_map(|line| {
            crate::transcript_layout::wrap_line(
                Line::raw(line.to_owned()),
                usize::from(width.max(1)),
            )
        })
        .collect()
}

fn details(menu: &McpState) -> String {
    let Some(server) = menu.server() else {
        return "No server selected.".into();
    };
    let mut text = format!("{}\n\nSaved: {}\nExposure: {}\nTransport: {}\nSource: {}{}\nTool overrides: {}\nRegistered tools: {}\n\n{}",
        server.name, if server.enabled { "enabled" } else { "disabled" }, server.exposure,
        server.transport, server.source, if server.writable { "" } else { " (read-only)" },
        server.overrides, server.tool_count, server.description);
    if matches!(menu.view, McpView::Tools(_) | McpView::Detail(_)) {
        let index = if matches!(menu.view, McpView::Tools(_)) {
            Some(menu.pos)
        } else {
            menu.detail_tool
        };
        if let Some(tool) = index.and_then(|index| server.tools.get(index)) {
            text.push_str(&format!(
                "\n\n{}\nExposure: {}\n{}\n\n{}\n\n{}",
                tool.name, tool.exposure, tool.annotations, tool.description, tool.schema
            ));
        }
        if server.tools.len() < server.tool_count {
            text.push_str(
                "\n\nTool metadata is truncated; refresh or inspect natively for the full catalog.",
            );
        }
    }
    text.push_str("\n\nNative status (last explicit refresh):\n");
    text.push_str(if menu.native_status.is_empty() {
        "Not observed yet."
    } else {
        &menu.native_status
    });
    text
}

pub(super) fn render(
    frame: &mut Frame,
    menu: &mut McpState,
    config: &Config,
    theme: &Theme,
    reload_required: bool,
) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    for cell in &mut frame.buffer_mut().content {
        let bg = if cell.bg == ratatui::style::Color::Reset {
            theme.bg
        } else {
            cell.bg
        };
        let fg = crate::color::lerp_rgb(cell.fg, bg, 0.6);
        cell.set_bg(bg).set_fg(fg);
        cell.modifier.insert(Modifier::DIM);
    }
    let compact = matches!(
        menu.view,
        McpView::Exposure(_) | McpView::Confirm { .. } | McpView::Login
    );
    let width = area
        .width
        .saturating_sub(if area.width > 6 { 6 } else { 0 })
        .min(if compact { 80 } else { 116 });
    let height = area
        .height
        .saturating_sub(if area.height > 4 { 4 } else { 0 })
        .min(if compact { 22 } else { 32 });
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let shadow = Rect::new(
        (popup.x + 1).min(area.right()),
        (popup.y + 1).min(area.bottom()),
        width.min(area.right().saturating_sub(popup.x + 1)),
        height.min(area.bottom().saturating_sub(popup.y + 1)),
    );
    frame.render_widget(
        Block::default().style(theme.overlay.background.style().add_modifier(Modifier::DIM)),
        shadow,
    );
    frame.render_widget(Clear, popup);
    let border = Block::bordered()
        .title(crate::i18n::tr(config.language, "mcp.title"))
        .title_alignment(Alignment::Center)
        .style(theme.overlay.background.style())
        .border_style(theme.overlay.border.style());
    let inner = border.inner(popup);
    frame.render_widget(border, popup);
    if inner.width < 2 || inner.height < 2 {
        return;
    }
    let content = Rect::new(inner.x + 1, inner.y, inner.width - 2, inner.height);
    let header = Rect::new(content.x, content.y, content.width, 1);
    frame.render_widget(
        Paragraph::new(format!(
            "{}{}",
            if menu.pending.is_some() {
                "Working…  "
            } else {
                ""
            },
            if reload_required {
                "Saved settings pending /reload"
            } else {
                "Configuration · on-demand native inspection"
            }
        ))
        .style(theme.overlay.muted.style()),
        header,
    );
    let footer_height = content.height.saturating_sub(2).min(5);
    let body = Rect::new(
        content.x,
        content.y + 1,
        content.width,
        content.height.saturating_sub(footer_height + 1),
    );
    menu.viewport_rows = usize::from(body.height).max(1);
    let sidebar = body.width >= 104 && matches!(menu.view, McpView::Servers | McpView::Tools(_));
    let list = if sidebar {
        Rect::new(body.x, body.y, body.width - 49, body.height)
    } else {
        body
    };
    let rows: Vec<String> = match &menu.view {
        McpView::Servers => {
            let servers = menu.visible_servers();
            if servers.is_empty() {
                vec![if menu.snapshot.servers.is_empty() {
                    "No configured MCP servers.".into()
                } else {
                    "No matching servers.".into()
                }]
            } else {
                servers
                    .iter()
                    .map(|server| {
                        format!(
                            "{}  {} · {} · {} tools",
                            server.name,
                            if server.enabled {
                                "enabled"
                            } else {
                                "disabled"
                            },
                            server.exposure,
                            server.tool_count
                        )
                    })
                    .collect()
            }
        }
        McpView::Tools(_) => menu
            .server()
            .map(|server| {
                if server.tools.is_empty() {
                    vec![if server.tool_count == 0 {
                        "No currently registered tools. Refresh after connection.".into()
                    } else {
                        "Tool definitions exceed the display metadata budget.".into()
                    }]
                } else {
                    server
                        .tools
                        .iter()
                        .map(|tool| format!("{}  [{}]", tool.name, tool.exposure))
                        .collect()
                }
            })
            .unwrap_or_default(),
        McpView::Exposure(_) => menu
            .snapshot
            .exposures
            .iter()
            .map(|mode| format!("{} · {}", mode.name, mode.description))
            .collect(),
        McpView::Confirm { server, patch } => vec![
            format!(
                "Confirm {} for {}?",
                match patch {
                    Some(McpPatch::Enabled(true)) => "enable".into(),
                    Some(McpPatch::Enabled(false)) => "disable".into(),
                    Some(McpPatch::Exposure(mode)) => format!("exposure = {mode}"),
                    None => "native sign-out".into(),
                },
                server
            ),
            String::new(),
            "Cancel".into(),
            "Save / confirm".into(),
            String::new(),
            if patch.is_some() {
                "Running: save only; /reload is required later.".into()
            } else {
                "Idle only: remove the native MCP credential.".into()
            },
            if patch.is_some() {
                "Idle: save and automatically reload extensions and other resources.".into()
            } else {
                "No configuration write or resource reload.".into()
            },
        ],
        McpView::Detail(_) => vec![],
        McpView::Login => vec![
            "Native MCP sign-in · callback input is masked.".into(),
            "Closing cancels the pending native input.".into(),
        ],
    };
    if matches!(menu.view, McpView::Detail(_) | McpView::Login) {
        let content = if matches!(menu.view, McpView::Login) {
            format!("Native MCP sign-in\n\n{}\n\n{}\n\nCallback input is masked and discarded on submit or cancel.", menu.notice, menu.authorization_url.as_deref().unwrap_or_default())
        } else {
            details(menu)
        };
        let lines = wrapped(&content, list.width);
        menu.detail_scroll = menu
            .detail_scroll
            .min(lines.len().saturating_sub(usize::from(list.height)));
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(menu.detail_scroll)
                    .take(usize::from(list.height))
                    .collect::<Vec<_>>(),
            )
            .style(theme.overlay.text.style()),
            list,
        );
    } else {
        let confirmation = matches!(menu.view, McpView::Confirm { .. });
        let selected = if confirmation { menu.pos + 2 } else { menu.pos };
        let start = if confirmation {
            0
        } else {
            selected.saturating_sub(usize::from(list.height).saturating_sub(1))
        };
        let lines = rows
            .into_iter()
            .enumerate()
            .skip(start)
            .take(usize::from(list.height))
            .map(|(index, row)| {
                let is_selected = index == selected && (!confirmation || index == 2 || index == 3);
                Line::styled(
                    format!("{} {row}", if is_selected { "›" } else { " " }),
                    if is_selected {
                        theme.overlay.selection.style()
                    } else {
                        theme.overlay.text.style()
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines).style(theme.overlay.background.style()),
            list,
        );
    }
    if sidebar {
        let rule = Rect::new(list.right() + 1, body.y, 1, body.height);
        frame.render_widget(
            Paragraph::new(vec![Line::raw("│"); usize::from(rule.height)])
                .style(theme.overlay.border.style()),
            rule,
        );
        let pane = Rect::new(
            rule.right() + 1,
            body.y,
            body.right().saturating_sub(rule.right() + 1),
            body.height,
        );
        let lines = wrapped(&details(menu), pane.width);
        menu.detail_scroll = menu
            .detail_scroll
            .min(lines.len().saturating_sub(usize::from(pane.height)));
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(menu.detail_scroll)
                    .take(usize::from(pane.height))
                    .collect::<Vec<_>>(),
            )
            .style(theme.overlay.text.style()),
            pane,
        );
    }
    let footer = Rect::new(content.x, body.bottom(), content.width, footer_height);
    let editing = menu.editor.as_ref().map(|editor| {
        if editor.secret {
            format!(
                "Callback: {}▏",
                "•".repeat(editor.buf.chars().count().min(64))
            )
        } else {
            format!("Filter: {}▏", editor.buf)
        }
    });
    let authorization_hint = if menu.authorization_url.is_some() {
        crate::help::key_hints(config, Scope::Mcp, &[Action::OpenLink, Action::CopyLink])
    } else {
        String::new()
    };
    let hint = if menu.editor.is_some() {
        crate::help::key_hints(config, Scope::PageEdit, &[Action::Confirm, Action::Cancel])
    } else {
        crate::help::key_hints(
            config,
            Scope::Mcp,
            &[
                Action::Inspect,
                Action::Reconnect,
                Action::Login,
                Action::EditExposure,
                Action::ToggleEnabled,
                Action::Logout,
                Action::Refresh,
                Action::Close,
                Action::Back,
            ],
        )
    };
    let notice = if menu.notice.is_empty() {
        if menu.snapshot.trusted {
            "Project configuration is trusted."
        } else {
            "Project configuration is not read: workspace is untrusted."
        }
    } else {
        &menu.notice
    };
    let hint = format!("{authorization_hint}  {hint}");
    let mut lines = wrapped(&hint, footer.width);
    lines.truncate(2);
    lines.extend(
        wrapped(
            editing
                .as_deref()
                .unwrap_or(&format!("Filter: {}", menu.filter)),
            footer.width,
        )
        .into_iter()
        .take(1),
    );
    lines.extend(wrapped(notice, footer.width).into_iter().take(2));
    frame.render_widget(
        Paragraph::new(lines).style(theme.overlay.muted.style()),
        footer,
    );
}
