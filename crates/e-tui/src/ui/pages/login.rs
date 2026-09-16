use super::*;

pub(crate) fn render_login(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    theme: &Theme,
    config: &crate::Config,
) {
    let mut viewport = crate::input_page::ViewportState::default();
    render_login_scrolled(frame, area, login, &mut viewport, theme, config);
}

pub(super) fn render_login_scrolled(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    login: &LoginState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
    config: &crate::Config,
) {
    let language = config.language;
    let regions = input_page_shell(frame, area, theme);
    let buffer = frame.buffer_mut();
    let title_key = match &login.page {
        crate::login::Page::Menu => "input_page.login.title",
        crate::login::Page::Providers => "input_page.login.providers_title",
        crate::login::Page::ApiKey { .. } => "input_page.login.api_key_title",
        crate::login::Page::ProxyList => "input_page.login.proxy_list_title",
        crate::login::Page::ProxyForm => "input_page.login.proxy_form_title",
        crate::login::Page::ProxyDelete { .. } => "input_page.login.proxy_delete_title",
        crate::login::Page::NativeProviders => {
            if login.auth_logout {
                "input_page.login.native_logout_title"
            } else {
                "input_page.login.native_provider_title"
            }
        }
        crate::login::Page::NativeMethods { .. } => "input_page.login.native_method_title",
        crate::login::Page::NativeLogout { .. } => "input_page.login.native_logout_title",
        crate::login::Page::NativePrompt(_)
        | crate::login::Page::NativeWaiting { .. }
        | crate::login::Page::NativeOutcome { .. } => "input_page.login.native_auth_title",
    };
    let title = crate::i18n::tr(language, title_key);
    buffer.set_line(
        regions.header.x,
        regions.header.y,
        &Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.input.hint.fg)),
            Span::styled(
                if login.auth_logout {
                    "/logout"
                } else {
                    "/login"
                },
                Style::default().fg(theme.fg),
            ),
            Span::styled(format!("  {title}"), Style::default().fg(theme.dim)),
        ]),
        regions.header.width,
    );

    let item_area = regions.body;
    let area = item_area;
    let body = native_page_body(login, language);
    let total_rows = match &login.page {
        crate::login::Page::Menu => 2,
        crate::login::Page::Providers => login.providers.len(),
        crate::login::Page::ProxyList => login.proxies.len() + 1,
        crate::login::Page::ProxyForm => crate::login::PROXY_ROWS,
        crate::login::Page::ProxyDelete { .. } => 3,
        crate::login::Page::NativeProviders | crate::login::Page::NativeMethods { .. } => {
            login.row_count()
        }
        crate::login::Page::NativeLogout { .. } => login.row_count() + 1,
        _ if !body.rows.is_empty() => body.rows.len(),
        crate::login::Page::NativePrompt(_) => 2,
        _ => 1,
    };
    let focused_row = body
        .rows
        .iter()
        .position(|row| row.action == Some(login.pos))
        .unwrap_or(login.pos);
    let visible = usize::from(item_area.height).saturating_sub(usize::from(body.header.is_some()));
    viewport.ensure_visible(focused_row, visible, total_rows);
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
        crate::login::Page::NativeProviders => {
            for (i, provider) in login.auth_providers.iter().enumerate().skip(start) {
                let y = item_area.y + i.saturating_sub(start) as u16;
                if y >= item_area.y + item_area.height {
                    break;
                }
                let available = LoginState::provider_actionable(provider, login.auth_logout);
                let status = if provider.configured {
                    provider.source.clone().unwrap_or_else(|| {
                        crate::i18n::tr(language, "input_page.login.native_configured")
                    })
                } else if available {
                    crate::i18n::tr(language, "input_page.login.native_available")
                } else {
                    crate::i18n::tr(language, "input_page.login.native_unavailable")
                };
                login_list_row(
                    buffer,
                    area,
                    y,
                    i == login.pos && available,
                    false,
                    &provider.name,
                    Span::styled(
                        status,
                        Style::default().fg(if provider.configured {
                            theme.ok
                        } else {
                            theme.dim
                        }),
                    ),
                    theme,
                );
            }
            if login.auth_providers.is_empty() && !login.loading {
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
        crate::login::Page::NativeMethods { provider } => {
            if let Some(provider) = login
                .auth_providers
                .iter()
                .find(|candidate| candidate.id == *provider)
            {
                for (i, method) in provider.methods.iter().enumerate().skip(start) {
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
                        &method.name,
                        Span::styled(
                            method.description.clone().unwrap_or_default(),
                            Style::default().fg(theme.dim),
                        ),
                        theme,
                    );
                }
            }
        }
        crate::login::Page::NativeLogout { provider } => {
            let name = login
                .auth_providers
                .iter()
                .find(|candidate| candidate.id == *provider)
                .map(|candidate| candidate.name.as_str())
                .unwrap_or(provider);
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(
                    crate::i18n::tr_args(
                        language,
                        "input_page.login.native_logout_prompt",
                        &[("name", name.to_owned())],
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
                    Span::raw(""),
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
                    &crate::i18n::tr(language, "input_page.login.native_remove"),
                    Span::styled("", Style::default().fg(theme.err)),
                    theme,
                );
            }
        }
        crate::login::Page::NativePrompt(prompt)
            if prompt.kind == crate::agent::AuthPromptKind::Select
                || (prompt.kind == crate::agent::AuthPromptKind::ManualCode
                    && login.editing.is_none()) =>
        {
            render_native_body(buffer, area, item_area, start, &body, login.pos, theme);
        }
        crate::login::Page::NativePrompt(prompt) => {
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(
                    prompt.message.clone(),
                    Style::default().fg(theme.fg),
                )),
                item_area.width,
            );
            if item_area.height > 1 {
                let value = login.editing.as_deref().unwrap_or("");
                let shown = if matches!(
                    prompt.kind,
                    crate::agent::AuthPromptKind::Secret | crate::agent::AuthPromptKind::ManualCode
                ) {
                    "●".repeat(value.chars().count())
                } else {
                    value.to_owned()
                };
                buffer.set_line(
                    area.x,
                    item_area.y + 1,
                    &Line::from(Span::styled(
                        format!("{shown}█"),
                        Style::default().fg(theme.ok),
                    )),
                    item_area.width,
                );
            }
        }
        crate::login::Page::NativeWaiting { .. } => {
            render_native_body(buffer, area, item_area, start, &body, login.pos, theme);
        }
        crate::login::Page::NativeOutcome { outcome, message } => {
            let color = if *outcome == crate::agent::AuthOutcomeKind::Succeeded {
                theme.ok
            } else {
                theme.err
            };
            buffer.set_line(
                area.x,
                item_area.y,
                &Line::from(Span::styled(message.clone(), Style::default().fg(color))),
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
        page_key_hints(config, login.key_scope())
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

/// Body of a native page: an optional fixed header line plus the rows that
/// scroll under it. The cursor space is the row space, so row counts, focus,
/// and rendering all agree with `LoginState::row_count`.
struct NativePageBody {
    header: Option<String>,
    rows: Vec<NativeRow>,
}

struct NativeRow {
    text: String,
    detail: String,
    action: Option<usize>,
    kind: NativeRowKind,
}

enum NativeRowKind {
    Notice,
    Action,
}

fn native_page_body(login: &LoginState, language: crate::Language) -> NativePageBody {
    match &login.page {
        crate::login::Page::NativePrompt(prompt)
            if prompt.kind == crate::agent::AuthPromptKind::Select =>
        {
            NativePageBody {
                header: Some(prompt.message.clone()),
                rows: prompt
                    .options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| NativeRow {
                        text: option.label.clone(),
                        detail: option.description.clone().unwrap_or_default(),
                        action: Some(index),
                        kind: NativeRowKind::Action,
                    })
                    .collect(),
            }
        }
        crate::login::Page::NativePrompt(prompt)
            if prompt.kind == crate::agent::AuthPromptKind::ManualCode
                && login.editing.is_none() =>
        {
            let mut rows = native_notice_rows(login, language);
            let mut actions = native_action_rows(login, language);
            let manual = actions.len();
            actions.push(NativeRow {
                text: crate::i18n::tr(language, "input_page.login.native_enter_manual"),
                detail: String::new(),
                action: Some(manual),
                kind: NativeRowKind::Action,
            });
            rows.extend(actions);
            NativePageBody {
                header: Some(prompt.message.clone()),
                rows,
            }
        }
        crate::login::Page::NativeWaiting { .. } => {
            let mut rows = native_notice_rows(login, language);
            rows.push(NativeRow {
                text: crate::i18n::tr(language, "input_page.login.native_waiting"),
                detail: String::new(),
                action: None,
                kind: NativeRowKind::Notice,
            });
            rows.extend(native_action_rows(login, language));
            NativePageBody { header: None, rows }
        }
        _ => NativePageBody {
            header: None,
            rows: Vec::new(),
        },
    }
}

fn native_notice_rows(login: &LoginState, language: crate::Language) -> Vec<NativeRow> {
    let mut rows = Vec::new();
    for notice in &login.auth_notices {
        rows.extend(notice.message.lines().map(|line| NativeRow {
            text: line.to_owned(),
            detail: String::new(),
            action: None,
            kind: NativeRowKind::Notice,
        }));
        if let Some(url) = notice.url.as_ref() {
            rows.push(NativeRow {
                text: url.clone(),
                detail: String::new(),
                action: None,
                kind: NativeRowKind::Notice,
            });
        }
        if let Some(code) = notice.code.as_ref() {
            rows.push(NativeRow {
                text: crate::i18n::tr_args(
                    language,
                    "input_page.login.native_device_code",
                    &[("code", code.clone())],
                ),
                detail: String::new(),
                action: None,
                kind: NativeRowKind::Notice,
            });
        }
    }
    rows
}

fn native_action_rows(login: &LoginState, language: crate::Language) -> Vec<NativeRow> {
    login
        .native_notice_actions()
        .into_iter()
        .enumerate()
        .map(|(index, action)| {
            let key = match action {
                crate::login::NativeNoticeAction::OpenUrl(_) => "input_page.login.native_open_url",
                crate::login::NativeNoticeAction::CopyUrl(_) => "input_page.login.native_copy_url",
                crate::login::NativeNoticeAction::CopyCode(_) => {
                    "input_page.login.native_copy_code"
                }
            };
            NativeRow {
                text: crate::i18n::tr(language, key),
                detail: String::new(),
                action: Some(index),
                kind: NativeRowKind::Action,
            }
        })
        .collect()
}

fn render_native_body(
    buffer: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    item_area: ratatui::layout::Rect,
    start: usize,
    body: &NativePageBody,
    pos: usize,
    theme: &Theme,
) {
    let offset = usize::from(body.header.is_some()) as u16;
    if let Some(header) = body.header.as_ref() {
        buffer.set_line(
            area.x,
            item_area.y,
            &Line::from(Span::styled(header.clone(), Style::default().fg(theme.fg))),
            item_area.width,
        );
    }
    for (index, row) in body.rows.iter().enumerate().skip(start) {
        let y = item_area.y + offset + index.saturating_sub(start) as u16;
        if y >= item_area.y + item_area.height {
            break;
        }
        match row.kind {
            NativeRowKind::Notice => {
                buffer.set_line(
                    area.x,
                    y,
                    &Line::from(Span::styled(
                        row.text.clone(),
                        Style::default().fg(theme.dim),
                    )),
                    item_area.width,
                );
            }
            NativeRowKind::Action => login_list_row(
                buffer,
                area,
                y,
                row.action == Some(pos),
                false,
                &row.text,
                Span::styled(row.detail.clone(), Style::default().fg(theme.dim)),
                theme,
            ),
        }
    }
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
        value.style = value.style.fg(theme.ok).add_modifier(Modifier::BOLD);
    }
    spans.push(value);
    let line = Line::from(spans);
    let line_width = (line.width() as u16).min(area.width);
    buffer.set_line(area.x, y, &line, line_width);
}
