use super::*;

pub(crate) fn render_login(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    theme: &Theme,
    language: crate::Language,
) {
    let mut viewport = crate::input_page::ViewportState::default();
    render_login_scrolled(frame, area, login, &mut viewport, theme, language);
}

pub(super) fn render_login_scrolled(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
    language: crate::Language,
) {
    let regions = input_page_shell(frame, area, theme);
    let buffer = frame.buffer_mut();
    let title_key = match &login.page {
        crate::login::Page::Menu => "input_page.login.title",
        crate::login::Page::Providers => "input_page.login.providers_title",
        crate::login::Page::ApiKey { .. } => "input_page.login.api_key_title",
        crate::login::Page::ProxyList => "input_page.login.proxy_list_title",
        crate::login::Page::ProxyForm => "input_page.login.proxy_form_title",
        crate::login::Page::ProxyDelete { .. } => "input_page.login.proxy_delete_title",
    };
    let title = crate::i18n::tr(language, title_key);
    buffer.set_line(
        regions.header.x,
        regions.header.y,
        &Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.input.hint.fg)),
            Span::styled("/login", Style::default().fg(theme.fg)),
            Span::styled(format!("  {title}"), Style::default().fg(theme.dim)),
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
                (
                    "input_page.login.api_key.label",
                    "input_page.login.api_key.description",
                ),
                (
                    "input_page.login.proxy.label",
                    "input_page.login.proxy.description",
                ),
            ];
            for (i, (label_key, hint_key)) in items.iter().enumerate().skip(start) {
                let label = crate::i18n::tr(language, label_key);
                let hint = crate::i18n::tr(language, hint_key);
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
                    &label,
                    Span::styled(hint, Style::default().fg(theme.dim)),
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
                        crate::i18n::tr_args(
                            language,
                            "input_page.login.configured",
                            &[("hint", hint.to_owned())],
                        ),
                        Style::default().fg(theme.ok),
                    )
                } else {
                    Span::styled(
                        crate::i18n::tr(language, "input_page.login.not_configured"),
                        Style::default().fg(theme.dim),
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
                        crate::i18n::tr(language, "input_page.login.no_providers"),
                        Style::default().fg(theme.dim),
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
                &Line::from(Span::styled(shown, Style::default().fg(theme.ok))),
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
                    Span::styled(p.base_url.clone(), Style::default().fg(theme.dim)),
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
                    &crate::i18n::tr(language, "input_page.login.new_proxy"),
                    Span::styled(
                        crate::i18n::tr(language, "input_page.login.new_proxy_description"),
                        Style::default().fg(theme.user),
                    ),
                    theme,
                );
            }
        }
        crate::login::Page::ProxyForm => {
            let fields: &[&str] = &[
                "input_page.login.field.base_url",
                "input_page.login.field.api_key",
                "input_page.login.field.protocol",
                "input_page.login.field.model",
            ];
            for (i, label_key) in fields.iter().enumerate().skip(start) {
                let label = crate::i18n::tr(language, label_key);
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
                    Span::styled(format!("{shown}█"), Style::default().fg(theme.ok))
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
                        crate::i18n::tr(language, "input_page.login.optional")
                    } else if i == 1 {
                        "●".repeat(v.chars().count())
                    } else {
                        v
                    };
                    Span::styled(shown, Style::default().fg(theme.dim))
                };
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos,
                    editing,
                    &label,
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
                    &crate::i18n::tr(language, "input_page.login.save"),
                    Span::styled(
                        crate::i18n::tr(language, "input_page.login.save_description"),
                        Style::default().fg(theme.user),
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
                    crate::i18n::tr_args(
                        language,
                        "input_page.login.delete_prompt",
                        &[("name", name.clone())],
                    ),
                    Style::default().fg(theme.fg),
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
                    &crate::i18n::tr(language, "common.cancel"),
                    Span::styled(
                        crate::i18n::tr(language, "input_page.login.keep_proxy"),
                        Style::default().fg(theme.dim),
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
                    &crate::i18n::tr(language, "common.delete"),
                    Span::styled(
                        crate::i18n::tr(language, "input_page.login.delete_permanently"),
                        Style::default().fg(theme.err),
                    ),
                    theme,
                );
            }
        }
    }

    // Footer: the last rejected write outranks the hint.
    let footer = if let Some(error) = login.error.as_deref() {
        format!("✗ {error}")
    } else if login.loading {
        crate::i18n::tr(language, "input_page.login.loading")
    } else {
        match &login.page {
            crate::login::Page::Menu => crate::i18n::tr(language, "input_page.login.footer.menu"),
            crate::login::Page::Providers => {
                crate::i18n::tr(language, "input_page.login.footer.providers")
            }
            crate::login::Page::ApiKey { .. } => {
                crate::i18n::tr(language, "input_page.login.footer.api_key")
            }
            crate::login::Page::ProxyList => {
                crate::i18n::tr(language, "input_page.login.footer.proxy_list")
            }
            crate::login::Page::ProxyForm => {
                crate::i18n::tr(language, "input_page.login.footer.proxy_form")
            }
            crate::login::Page::ProxyDelete { .. } => {
                crate::i18n::tr(language, "input_page.login.footer.proxy_delete")
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
        &Line::from(Span::styled(footer, Style::default().fg(fg))),
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
    let style = input_page_item_style(theme, focused, false);
    let label_width = UnicodeWidthStr::width(label);
    let mut spans: Vec<Span<'static>> = vec![Span::styled(label.to_string(), style)];
    if label_width < 12 {
        spans.push(Span::styled(" ".repeat(12 - label_width), style));
    }
    let mut value = value;
    if focused || editing {
        value.style = value.style.fg(theme.ok);
    }
    spans.push(value);
    let line = Line::from(spans);
    let line_width = (line.width() as u16).min(area.width);
    buffer.set_line(area.x, y, &line, line_width);
}
