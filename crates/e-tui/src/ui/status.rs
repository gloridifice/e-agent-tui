use super::*;
use crate::ui::component::status;

pub(super) fn render_status(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &TuiApp,
    _scroll: &ScrollState,
    theme: &Theme,
) {
    let dim = status::dim(theme);
    // Running bullet leads the status bar: yellow breathing while the agent
    // is running (or has just been sent work), gray while idle. One space
    // separates it from the elements that follow.
    let drafting = state.session.new_conversation.is_some();
    let bullet =
        if !drafting && (state.session.status == AgentStatus::Running || state.session.working) {
            Span::styled(
                "•",
                Style::default().fg(breathing_color(theme, state.breath_phase())),
            )
        } else {
            Span::styled("•", dim)
        };
    let mode = state
        .session
        .new_conversation
        .as_ref()
        .map(|draft| draft.mode.as_str())
        .or(state.session.current_mode.as_deref())
        .unwrap_or(state.config.default_mode.as_str());
    let mut left_spans = vec![
        bullet,
        Span::styled(" ", dim),
        Span::styled(mode.to_owned(), dim),
    ];
    if let Some(model) = state
        .session
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
    {
        left_spans.push(Span::styled(" ", dim));
        left_spans.push(Span::styled(model.to_owned(), dim));
    }
    if let Some(rate) = (!drafting).then(|| state.cache_hit_rate()).flatten() {
        left_spans.push(Span::styled(" ", dim));
        left_spans.push(Span::styled(format!("CH{rate}%"), dim));
    }
    // Reasoning effort rides after the cache-hit rate, styled identically to
    // the model and CH entries, and is omitted entirely when the current route
    // exposes no reasoning metadata.
    if let Some(status) = state.catalogs.effort_status() {
        let label = status
            .label
            .unwrap_or_else(|| crate::i18n::tr(state.config.language, "status.effort_default"));
        left_spans.push(Span::styled(" ", dim));
        left_spans.push(Span::styled(
            format!(
                "{}:{label}",
                crate::i18n::tr(state.config.language, "status.effort_prefix")
            ),
            dim,
        ));
    }
    let left = Line::from(left_spans);
    let right = Line::from(Span::styled(
        format!(
            "^h {}",
            crate::i18n::tr(state.config.language, "status.help")
        ),
        dim,
    ))
    .alignment(ratatui::layout::Alignment::Right);
    // Render through the buffer directly: no wrapping, hard clip at edges.
    let buffer = frame.buffer_mut();
    buffer.set_line(area.x, area.y, &left, area.width);
    // +4 slack: Line::width() under-counts CJK by a couple of cells vs the
    // buffer writer; without it the last characters would be clipped.
    let right_width = (right.width() + 4).min(area.width as usize) as u16;
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
    fn new_draft_keeps_the_attached_sessions_workspace_path() {
        let mut state = TuiApp::default();
        state.session.session_cwd = Some(r"D:\workspace\project".into());
        state.session.new_conversation = Some(crate::app::NewConversationDraft {
            mode: "code".into(),
            pending_input: None,
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
    fn status_bar_renders_effort_after_the_model_with_the_same_dim_style() {
        let mut state = TuiApp::default();
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
        assert!(
            effort_at > model_at,
            "effort must follow the model entry: {line:?}"
        );
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
