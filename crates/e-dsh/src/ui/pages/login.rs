use super::*;

pub(crate) fn render_login(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    theme: &Theme,
) {
    let mut viewport = crate::input_page::ViewportState::default();
    render_login_scrolled(frame, area, login, &mut viewport, theme);
}

pub(super) fn render_login_scrolled(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);
    let buffer = frame.buffer_mut();
    let title = match &login.page {
        crate::login::Page::Menu => "登录",
        crate::login::Page::Providers => "API key · 选择提供商",
        crate::login::Page::ApiKey { .. } => "API key · 填写",
        crate::login::Page::ProxyList => "Proxy · 已保存的代理",
        crate::login::Page::ProxyForm => "Proxy · 新建代理",
        crate::login::Page::ProxyDelete { .. } => "Proxy · 删除确认",
    };
    buffer.set_line(
        regions.header.x,
        regions.header.y,
        &Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user).bg(theme.bg_soft)),
            Span::styled(title, Style::default().fg(theme.fg).bg(theme.bg_soft)),
        ]),
        regions.header.width,
    );

    let item_area = regions.body;
    let area = item_area;
    let total_rows = match &login.page {
        crate::login::Page::Menu => 2,
        crate::login::Page::Providers => login.providers.len(),
        crate::login::Page::ProxyList => login.proxies.len() + 1,
        crate::login::Page::ProxyForm => crate::login::PROXY_ROWS,
        crate::login::Page::ProxyDelete { .. } => 3,
        _ => 1,
    };
    viewport.ensure_visible(login.pos, item_area.height as usize, total_rows);
    let start = viewport.start;
    match &login.page {
        crate::login::Page::Menu => {
            let items: &[(&str, &str)] = &[
                ("API key", "为各模型提供商填写 API key"),
                ("Proxy", "管理自定义代理端点"),
            ];
            for (i, (label, hint)) in items.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos,
                    false,
                    label,
                    Span::styled(
                        (*hint).to_string(),
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
        }
        crate::login::Page::Providers => {
            for (i, p) in login.providers.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                let value = if p.api_key_configured {
                    let hint = p.api_key_hint.as_deref().unwrap_or("");
                    Span::styled(
                        format!("已配置 {hint}"),
                        Style::default().fg(theme.ok).bg(theme.bg_soft),
                    )
                } else {
                    Span::styled(
                        "未配置（Enter 填写）",
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    )
                };
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos && p.api_key_writable,
                    false,
                    &p.name,
                    value,
                    theme,
                );
            }
            if login.providers.is_empty() && !login.loading {
                buffer.set_line(
                    area.x,
                    item_area.y,
                    &Line::from(Span::styled(
                        "没有可用的提供商",
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    )),
                    item_area.width,
                );
            }
        }
        crate::login::Page::ApiKey { .. } => {
            let buf = login.editing.as_deref().unwrap_or("");
            let shown = format!("{}█", "●".repeat(buf.chars().count()));
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(
                    shown,
                    Style::default().fg(theme.ok).bg(theme.bg),
                )),
                item_area.width,
            );
        }
        crate::login::Page::ProxyList => {
            for (i, p) in login.proxies.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos,
                    false,
                    &p.name,
                    Span::styled(
                        p.base_url.clone(),
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
            let new_index = login.proxies.len();
            let new_y = item_area.y + new_index.saturating_sub(start) as u16;
            if new_index >= start && new_y < item_area.y + item_area.height {
                login_list_row(
                    buffer,
                    area,
                    new_y,
                    login.pos == login.proxies.len(),
                    false,
                    "+ New",
                    Span::styled(
                        "新建代理".to_string(),
                        Style::default().fg(theme.user).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
        }
        crate::login::Page::ProxyForm => {
            let fields: &[&str] = &["base url", "api key", "协议模式", "模型名称"];
            for (i, label) in fields.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                let editing = i == login.pos && login.editing.is_some();
                let value = if editing {
                    let buf = login.editing.as_deref().unwrap_or("");
                    let shown = if i == 1 {
                        "●".repeat(buf.chars().count())
                    } else {
                        buf.to_string()
                    };
                    Span::styled(
                        format!("{shown}█"),
                        Style::default().fg(theme.ok).bg(theme.bg),
                    )
                } else {
                    let v = match i {
                        2 => crate::login::PROTOCOLS
                            [login.draft.protocol % crate::login::PROTOCOLS.len()]
                        .1
                        .to_string(),
                        _ => match i {
                            0 => login.draft.base_url.clone(),
                            1 => login.draft.api_key.clone(),
                            _ => login.draft.model.clone(),
                        },
                    };
                    let shown = if i == 2 {
                        format!("◄ {v} ►")
                    } else if v.is_empty() {
                        "（可选）".to_string()
                    } else if i == 1 {
                        "●".repeat(v.chars().count())
                    } else {
                        v
                    };
                    Span::styled(shown, Style::default().fg(theme.dim).bg(theme.bg_soft))
                };
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos,
                    editing,
                    label,
                    value,
                    theme,
                );
            }
            let save_index = crate::login::PROXY_SAVE_ROW;
            let save_y = item_area.y + save_index.saturating_sub(start) as u16;
            if save_index >= start && save_y < item_area.y + item_area.height {
                login_list_row(
                    buffer,
                    area,
                    save_y,
                    login.pos == crate::login::PROXY_SAVE_ROW,
                    false,
                    "保存",
                    Span::styled(
                        "保存并创建".to_string(),
                        Style::default().fg(theme.user).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
        }
        crate::login::Page::ProxyDelete { name, .. } => {
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(
                    format!("确定删除代理“{name}”？"),
                    Style::default().fg(theme.fg).bg(theme.bg_soft),
                )),
                item_area.width,
            );
            if item_area.height > 1 {
                login_list_row(
                    buffer,
                    area,
                    item_area.y + 1,
                    login.pos == 0,
                    false,
                    "取消",
                    Span::styled(
                        "保留该代理",
                        Style::default().fg(theme.dim).bg(theme.bg_soft),
                    ),
                    theme,
                );
            }
            if item_area.height > 2 {
                login_list_row(
                    buffer,
                    area,
                    item_area.y + 2,
                    login.pos == 1,
                    false,
                    "删除",
                    Span::styled("永久删除", Style::default().fg(theme.err).bg(theme.bg_soft)),
                    theme,
                );
            }
        }
    }

    // Footer: the last rejected write outranks the hint.
    let footer = if let Some(error) = login.error.as_deref() {
        format!("✗ {error}")
    } else if login.loading {
        "读取中…".to_string()
    } else {
        match &login.page {
            crate::login::Page::Menu => "↑/↓ 选择   Enter 进入   Esc 退出".to_string(),
            crate::login::Page::Providers => "↑/↓ 选择   Enter 填写   Esc 返回".to_string(),
            crate::login::Page::ApiKey { .. } => "Enter 保存   Esc 返回 · 密钥不会回显".to_string(),
            crate::login::Page::ProxyList => "↑/↓ 选择   Enter 执行   Esc 返回".to_string(),
            crate::login::Page::ProxyForm => {
                "↑/↓ 选字段   Enter 编辑/切换   Enter 保存   Esc 返回".to_string()
            }
            crate::login::Page::ProxyDelete { .. } => {
                "←/→ 选择   Enter 确认   Esc 取消".to_string()
            }
        }
    };
    let fg = if login.error.is_some() {
        theme.err
    } else {
        theme.dim
    };
    buffer.set_line(
        regions.footer.x,
        regions.footer.y,
        &Line::from(Span::styled(
            footer,
            Style::default().fg(fg).bg(theme.bg_soft),
        )),
        regions.footer.width,
    );
}

/// Render one login list row: a fixed-width label column followed by a value.
fn login_list_row(
    buffer: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    y: u16,
    focused: bool,
    editing: bool,
    label: &str,
    value: Span<'static>,
    theme: &Theme,
) {
    let name_bg = if focused && !editing {
        theme.bg
    } else {
        theme.bg_soft
    };
    let label_width = UnicodeWidthStr::width(label);
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        label.to_string(),
        Style::default().fg(theme.fg).bg(name_bg),
    )];
    if label_width < 12 {
        spans.push(Span::styled(
            " ".repeat(12 - label_width),
            Style::default().fg(theme.fg).bg(name_bg),
        ));
    }
    spans.push(value);
    let line = Line::from(spans);
    let line_width = (line.width() as u16).min(area.width);
    buffer.set_line(area.x, y, &line, line_width);
}
