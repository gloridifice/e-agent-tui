use super::*;
use crate::{app::lerp_color, ui::component::status};

const WORKING_INDICATOR_PHASE_OFFSET: f64 = 0.13;
const WORKING_INDICATOR_START_PHASE: f64 = 0.25;

fn working_indicator_spans(label: &str, theme: &Theme, phase: f64) -> Vec<Span<'static>> {
    format!("e·{label}")
        .chars()
        .enumerate()
        .map(|(index, character)| {
            let character_phase = phase + WORKING_INDICATOR_START_PHASE
                - index as f64 * WORKING_INDICATOR_PHASE_OFFSET;
            let level = ((character_phase * std::f64::consts::TAU).sin() + 1.0) / 2.0;
            let color = lerp_color(theme.working_status.running.fg, theme.coral, level);
            Span::styled(
                character.to_string(),
                Style::default().fg(color).add_modifier(Modifier::ITALIC),
            )
        })
        .collect()
}

fn format_tokens(count: u64) -> String {
    match count {
        0..=999 => count.to_string(),
        1_000..=9_999 => format!("{:.1}k", count as f64 / 1_000.0),
        10_000..=999_999 => format!("{}k", count.saturating_add(500) / 1_000),
        1_000_000..=9_999_999 => format!("{:.1}M", count as f64 / 1_000_000.0),
        _ => format!("{}M", count.saturating_add(500_000) / 1_000_000),
    }
}

pub(super) fn render_input_header(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &TuiApp,
    theme: &Theme,
) {
    crate::ui::component::rule::render(frame, area, theme);
    let available = usize::from(area.width.saturating_sub(6));
    if available == 0 || area.height == 0 {
        return;
    }
    let dim = status::dim(theme);
    // The italic frontend label carries a left-to-right sine wave while the
    // agent is working and stays dim while idle.
    let drafting = state.session.new_conversation.is_some();
    let indicator_text = format!("e·{}", state.frontend.label());
    let indicator = if state
        .session
        .new_conversation
        .as_ref()
        .is_some_and(|draft| draft.pending_input.is_some())
        || (!drafting && (state.session.status == AgentStatus::Running || state.session.working))
    {
        working_indicator_spans(state.frontend.label(), theme, state.breath_phase())
    } else {
        vec![Span::styled(
            indicator_text,
            dim.add_modifier(Modifier::ITALIC),
        )]
    };
    let mode = state
        .session
        .new_conversation
        .as_ref()
        .map(|draft| draft.mode.as_str())
        .or(state.session.current_mode.as_deref())
        .unwrap_or(state.config.default_mode.as_str());
    let mut left_spans = indicator;
    if state.session.session_id.is_none() {
        left_spans.push(Span::styled(" ", dim));
        left_spans.push(Span::styled(
            crate::i18n::tr(state.config.language, "common.loading"),
            dim,
        ));
    }
    if !mode.eq_ignore_ascii_case(state.frontend.label()) {
        left_spans.push(Span::styled(" ", dim));
        left_spans.push(Span::styled(mode.to_owned(), dim));
    }
    if let Some(model) = state
        .session
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
    {
        left_spans.push(Span::styled(" ", dim));
        let temporary = state
            .session
            .temporary_model
            .as_ref()
            .is_some_and(|temporary| {
                state.session.provider.as_deref() == Some(temporary.target.provider.as_str())
                    && model == temporary.target.model
            });
        let style = if temporary {
            dim.add_modifier(Modifier::ITALIC)
        } else {
            dim
        };
        let style = style.fg(state
            .render
            .status_flashes
            .model_color(theme, theme.input.status_hint.fg));
        left_spans.push(Span::styled(model.to_owned(), style));
    }
    let effort = state.catalogs.effort_status().map(|status| {
        let label = status
            .label
            .unwrap_or_else(|| crate::i18n::tr(state.config.language, "status.effort_default"));
        Span::styled(
            label,
            dim.fg(state
                .render
                .status_flashes
                .effort_color(theme, theme.input.status_hint.fg)),
        )
    });
    // Reserve effort before clipping the route, leaving both rule ends visible.
    let effort_width = effort.as_ref().map_or(0, |span| span.width() + 1);
    let mut label = crate::wrap::ellipsize_line(
        Line::from(left_spans),
        available.saturating_sub(effort_width),
    );
    if let Some(effort) = effort {
        if !label.spans.is_empty() {
            label.push_span(Span::styled(" ", dim));
        }
        label.push_span(effort);
    }
    let label = crate::wrap::ellipsize_line(label, available);
    let mut spans = vec![Span::styled(" ", dim)];
    spans.extend(label.spans);
    spans.push(Span::styled(" ", dim));
    frame
        .buffer_mut()
        .set_line(area.x + 2, area.y, &Line::from(spans), area.width - 4);
}

pub(super) fn render_status(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &TuiApp,
    _scroll: &ScrollState,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let dim = status::dim(theme);
    let drafting = state.session.new_conversation.is_some();
    let mut metrics = Vec::new();
    if let Some(rate) = (!drafting).then(|| state.cache_hit_rate()).flatten() {
        metrics.push(format!("CH{rate}%"));
    }
    if let Some(context_window) = (!drafting)
        .then(|| state.catalogs.current_model_context_window())
        .flatten()
    {
        let percent = if state.session.context_usage_unknown {
            "?".to_owned()
        } else {
            state
                .session
                .context_usage_percent(context_window)
                .to_string()
        };
        metrics.push(format!("{percent}%/{}", format_tokens(context_window)));
    }
    if let Some(cost) = (!drafting).then_some(state.session.cost_usd).flatten() {
        metrics.push(format!("${cost:.2}"));
    }
    let left = Line::from(Span::styled(metrics.join(" "), dim));
    let right_text = format!(
        "{} {}",
        state.config.key_mapping.label(
            crate::key_mapping::Scope::Global,
            crate::key_mapping::Action::PrintHelp
        ),
        crate::i18n::tr(state.config.language, "status.help")
    );
    let right_width = UnicodeWidthStr::width(right_text.as_str()).min(area.width as usize) as u16;
    let right = Line::from(Span::styled(right_text, dim));
    // Reserve the right label before clipping the left side so wide content
    // cannot overwrite or split the flush-right help hint.
    let left_width = area
        .width
        .saturating_sub(right_width.saturating_add(u16::from(right_width < area.width)));
    let buffer = frame.buffer_mut();
    buffer.set_line(area.x, area.y, &left, left_width);
    let right_x = area.x + area.width.saturating_sub(right_width);
    buffer.set_line(right_x, area.y, &right, right_width);
}

/// Session title row below the status bar: the latest `session/title` of
/// the attached session on the left and the session's workspace path on the
/// right. Overly long titles truncate with an ellipsis so the path stays
/// visible; the row is blank until the session has either.
pub(super) fn render_title(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &TuiApp,
    theme: &Theme,
) {
    let style = status::dim(theme);
    frame.render_widget(ratatui::widgets::Clear, area);
    let buffer = frame.buffer_mut();
    let width = area.width as usize;

    // Left-aligned title, truncated to leave the path (plus a small gap)
    // visible. Drawn first so the path below wins any overlap (defensive:
    // the truncation already reserves the path's columns).
    let title = if state.session.new_conversation.is_some() {
        crate::i18n::tr(state.config.language, "status.new_conversation")
    } else {
        match state.session.session_title.as_deref() {
            Some(title) if !title.trim().is_empty() => title.trim().to_owned(),
            _ => crate::i18n::tr(state.config.language, "status.new_session"),
        }
    };
    // The draft overlays the still-attached session: keep rendering that
    // session's workspace path until the next `welcome` replaces `session_cwd`.
    let cwd = state
        .session
        .session_cwd
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let path_w = UnicodeWidthStr::width(cwd.as_str());
    let avail = width.saturating_sub(path_w.saturating_add(2));
    let shown = if UnicodeWidthStr::width(title.as_str()) > avail {
        trim_to_width(&title, avail)
    } else {
        title
    };
    // `Buffer::set_line` does not pad a short line to the supplied width.
    // Write explicit trailing cells so a shorter title cannot leave glyphs
    // from the previous frame behind (especially after CJK wide cells).
    let shown_width = UnicodeWidthStr::width(shown.as_str());
    let padded = format!("{shown}{}", " ".repeat(width.saturating_sub(shown_width)));
    let left = Line::from(Span::styled(padded, style));
    buffer.set_line(area.x, area.y, &left, area.width);

    // Right-aligned workspace path (the session's header cwd), positioned by
    // its display width so it sits flush against the right edge. `set_line`
    // ignores `Line::alignment`, so the x offset is computed here instead.
    if !cwd.is_empty() {
        let right = Line::from(Span::styled(cwd, style));
        let right_x = area.x + area.width.saturating_sub(path_w as u16);
        buffer.set_line(right_x, area.y, &right, path_w as u16);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn status_bar_shows_known_session_cost_but_not_the_retained_draft_cost() {
        let mut state = TuiApp::default();
        state.session.session_id = Some("session".into());
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        for (cost, drafting, expected) in [
            (None, false, None),
            (Some(0.0), false, Some("$0.00")),
            (Some(0.123456), false, Some("$0.12")),
            (Some(0.126), false, Some("$0.13")),
            (Some(12.5), false, Some("$12.50")),
            (Some(12.5), true, None),
        ] {
            state.session.cost_usd = cost;
            state.session.new_conversation = drafting.then(|| crate::app::NewConversationDraft {
                mode: "standard".into(),
                pending_input: None,
                pending_card: None,
                attached: false,
                notice: None,
            });
            terminal
                .draw(|frame| {
                    render_status(
                        frame,
                        frame.area(),
                        &state,
                        &ScrollState::default(),
                        &Theme::ferra(),
                    )
                })
                .unwrap();
            let line = (0..80)
                .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
                .collect::<String>();
            if let Some(expected) = expected {
                assert!(line.contains(expected), "{line:?}");
            } else {
                assert!(!line.contains('$'), "{line:?}");
            }
            assert!(line.ends_with("Ctrl+H Help"), "{line:?}");
        }
    }

    #[test]
    fn status_bar_shows_loading_until_session_attachment() {
        for frontend in [crate::FrontendKind::Dsh, crate::FrontendKind::Pi] {
            for (language, loading) in [
                (crate::Language::English, "Loading…"),
                (crate::Language::SimplifiedChinese, "加载中…"),
            ] {
                let mut state = TuiApp::default();
                state.frontend = frontend;
                state.config.language = language;
                let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
                for attached in [false, true] {
                    state.session.session_id = attached.then(|| "session".into());
                    terminal
                        .draw(|frame| {
                            render_status(
                                frame,
                                frame.area(),
                                &state,
                                &ScrollState::default(),
                                &Theme::ferra(),
                            )
                        })
                        .unwrap();
                    let buffer = terminal.backend().buffer();
                    let line = (0..80).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
                    assert_eq!(
                        line.replace(' ', "").contains(loading),
                        !attached,
                        "status line: {line:?}"
                    );
                    assert!(line.starts_with(&format!("e·{} ", frontend.label())));
                    assert!(line.contains("Ctrl+H"), "help remains visible: {line:?}");
                }
            }
        }
    }

    #[test]
    fn temporary_model_status_is_italic_until_restoration() {
        let mut state = TuiApp::default();
        state.session.provider = Some("p".into());
        state.session.model = Some("luna".into());
        state.session.temporary_model = Some(crate::app::TemporaryModel {
            original: crate::agent::ModelSelection {
                provider: "p".into(),
                model: "base".into(),
                reasoning_effort: None,
            },
            target: crate::agent::ModelSelection {
                provider: "p".into(),
                model: "luna".into(),
                reasoning_effort: None,
            },
            phase: crate::app::TemporaryModelPhase::Active,
            materializing: false,
        });
        for temporary in [true, false] {
            if !temporary {
                state.session.temporary_model = None;
            }
            let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
            terminal
                .draw(|frame| {
                    render_status(
                        frame,
                        frame.area(),
                        &state,
                        &ScrollState::default(),
                        &Theme::ferra(),
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let line = (0..80).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
            let start = line[..line.find("luna").unwrap()].chars().count() as u16;
            for x in start..start + 4 {
                assert_eq!(
                    buffer[(x, 0)].modifier.contains(Modifier::ITALIC),
                    temporary
                );
            }
        }
    }

    #[test]
    fn new_draft_keeps_the_attached_sessions_workspace_path() {
        let mut state = TuiApp::default();
        state.session.session_cwd = Some(r"D:\workspace\project".into());
        state.session.new_conversation = Some(crate::app::NewConversationDraft {
            mode: "code".into(),
            pending_input: None,
            pending_card: None,
            attached: false,
            notice: None,
        });

        let mut terminal = Terminal::new(TestBackend::new(48, 1)).unwrap();
        terminal
            .draw(|frame| render_title(frame, frame.area(), &state, &Theme::ferra()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let line = (0..48)
            .map(|x| buffer[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>();
        // CJK glyphs occupy two cells; symbol() returns the second cell empty.
        let compact = line.replace(' ', "");
        assert!(
            compact.starts_with("Newconversation"),
            "draft title missing: {line:?}"
        );
        assert!(
            line.contains(r"D:\workspace\project"),
            "workspace path should stay visible while /new draft is pending: {line:?}"
        );
    }

    #[test]
    fn status_bar_renders_effort_and_context_after_the_model() {
        let mut state = TuiApp::default();
        state.session.model = Some("gpt".into());
        state.session.last_usage_sample = Some((
            Some(1),
            Some(1),
            crate::agent::timeline::TokenUsage {
                input_tokens: 80_000,
                output_tokens: 2_800,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        ));
        state.catalogs.current_model = Some(crate::agent::ModelSelection {
            provider: "openai".into(),
            model: "gpt".into(),
            reasoning_effort: Some("high".into()),
        });
        state.catalogs.model_providers = vec![crate::agent::ModelProvider {
            id: "openai".into(),
            name: "OpenAI".into(),
            models: vec![crate::agent::ModelDescriptor {
                id: "gpt".into(),
                name: "GPT".into(),
                description: None,
                context_window: Some(276_000),
                reasoning: Some(crate::agent::ModelReasoning {
                    efforts: vec![crate::agent::ReasoningEffort {
                        id: "high".into(),
                        name: "High".into(),
                        description: None,
                    }],
                    default_effort: None,
                }),
            }],
        }];

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    &state,
                    &ScrollState::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let line = (0..80)
            .map(|x| buffer[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>();
        let model_at = line.find("gpt").expect("model renders");
        let effort_at = line
            .find("Effort:High")
            .expect("effort renders after the model");
        let context_at = line
            .find("30%/276k")
            .expect("context usage renders after effort");
        assert!(
            effort_at > model_at && context_at > effort_at,
            "effort and context must follow the model entry: {line:?}"
        );
        state.session.context_usage_unknown = true;
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    &state,
                    &ScrollState::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let line = (0..80).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
        assert!(line.contains("?%/276k"), "post-compaction context: {line}");
        assert!(!line.contains("30%"));
    }

    #[test]
    fn status_bar_localizes_frontend_labels_without_changing_effort_names() {
        let mut state = TuiApp::default();
        state.config.language = crate::Language::SimplifiedChinese;
        state.session.model = Some("gpt".into());
        state.catalogs.current_model = Some(crate::agent::ModelSelection {
            provider: "openai".into(),
            model: "gpt".into(),
            reasoning_effort: Some("high".into()),
        });
        state.catalogs.model_providers = vec![crate::agent::ModelProvider {
            id: "openai".into(),
            name: "OpenAI".into(),
            models: vec![crate::agent::ModelDescriptor {
                id: "gpt".into(),
                name: "GPT".into(),
                description: None,
                context_window: None,
                reasoning: Some(crate::agent::ModelReasoning {
                    efforts: vec![crate::agent::ReasoningEffort {
                        id: "high".into(),
                        name: "High".into(),
                        description: None,
                    }],
                    default_effort: None,
                }),
            }],
        }];

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    &state,
                    &ScrollState::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();

        let compact = (0..80)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        assert!(
            compact.contains("推理:High"),
            "effort value changed: {compact}"
        );
        assert!(
            compact.contains("帮助"),
            "help label is not localized: {compact}"
        );
    }

    #[test]
    fn status_bar_uses_an_italic_frontend_indicator_and_flush_right_help() {
        let state = TuiApp::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    &state,
                    &ScrollState::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        for (x, symbol) in ["e", "·", "d", "s", "h"].into_iter().enumerate() {
            assert_eq!(buffer[(x as u16, 0)].symbol(), symbol);
            assert!(buffer[(x as u16, 0)].modifier.contains(Modifier::ITALIC));
        }
        assert_eq!(buffer[(79, 0)].symbol(), "p");
    }

    #[test]
    fn pi_indicator_replaces_the_redundant_pi_mode_text() {
        let mut state = TuiApp::default();
        state.frontend = crate::FrontendKind::Pi;
        state.session.session_id = Some("session".into());
        state.config.default_mode = "pi".into();
        state.session.current_mode = Some("pi".into());
        state.session.model = Some("model".into());
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    &state,
                    &ScrollState::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let line = (0..40).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
        assert!(line.starts_with("e·pi model"), "status line: {line:?}");
        assert!(
            !line.contains("e·pi pi"),
            "redundant mode remains: {line:?}"
        );
        for x in 0..4 {
            assert!(buffer[(x, 0)].modifier.contains(Modifier::ITALIC));
        }
        assert!(!buffer[(5, 0)].modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn status_bar_right_label_is_flush_for_cjk_text() {
        let mut state = TuiApp::default();
        state.config.language = crate::Language::SimplifiedChinese;

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    &state,
                    &ScrollState::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let expected_start = 80 - UnicodeWidthStr::width("Ctrl+H 帮助") as u16;
        let actual_start = (0..80u16)
            .find(|&x| buffer[(x, 0)].symbol() == "C")
            .expect("help label renders");
        assert_eq!(actual_start, expected_start);
    }

    #[test]
    fn status_bar_hides_effort_when_the_route_has_no_reasoning() {
        let mut state = TuiApp::default();
        state.session.model = Some("gpt".into());
        state.catalogs.current_model = Some(crate::agent::ModelSelection {
            provider: "openai".into(),
            model: "gpt".into(),
            reasoning_effort: None,
        });

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    &state,
                    &ScrollState::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let line = (0..80)
            .map(|x| buffer[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>();
        assert!(
            !line.contains("Effort"),
            "no effort placeholder when reasoning is absent: {line:?}"
        );
    }
}
