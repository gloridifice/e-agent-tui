//! Input Page renderers.

mod effort;
mod login;
mod model;
mod question;
mod resume;
mod settings;
mod theme;

use effort::render_effort_page;
pub(super) use login::render_login;
use login::render_login_scrolled;
use model::render_model_page;
use question::render_question_page;
use resume::render_resume_page;
pub(super) use settings::render_settings;
use theme::render_theme_page;

use super::*;
use crate::key_mapping::{Action as KeyAction, Scope as KeyScope};

#[derive(Clone, Copy)]
pub(super) struct InputPageRegions {
    pub(super) header: ratatui::layout::Rect,
    pub(super) body: ratatui::layout::Rect,
    pub(super) footer: ratatui::layout::Rect,
}

fn page_key_hints(config: &crate::Config, scope: KeyScope) -> String {
    use KeyAction::*;
    if scope == KeyScope::Page {
        let directions = [MoveLeft, MoveRight, MoveUp, MoveDown]
            .map(|action| config.key_mapping.label(scope, action))
            .join(" ");
        return format!(
            "{directions} {}   {}",
            crate::i18n::tr(config.language, "key.navigate"),
            crate::help::key_hints(config, scope, &[Confirm, Back])
        );
    }
    let actions: &[KeyAction] = match scope {
        KeyScope::PageChoice => &[Previous, Next, Confirm, Cancel],
        KeyScope::PageResume => &[Previous, Next, Confirm, Cancel],
        _ => &[Confirm, Cancel, Paste],
    };
    crate::help::key_hints(config, scope, actions)
}

pub(super) fn preferred_rows(page: &InputPageSession) -> usize {
    let body_rows = match &page.page {
        InputPage::Model(page) if !page.loading => {
            page.providers.len().max(page.active_models().len()).max(1)
        }
        InputPage::Effort(page)
            if !page.loading && !page.unavailable && !page.efforts.is_empty() =>
        {
            page.efforts.len() + usize::from(page.shows_current_default())
        }
        InputPage::Model(_) | InputPage::Effort(_) => 1,
        _ => return usize::MAX,
    };
    body_rows.saturating_add(6)
}

fn input_page_shell(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    theme: &Theme,
) -> InputPageRegions {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    render_ruled_line(frame, rows[0], theme);
    render_ruled_line(frame, rows[2], theme);
    render_ruled_line(frame, rows[5], theme);
    InputPageRegions {
        header: rows[1],
        body: rows[3],
        footer: rows[4],
    }
}

fn render_ruled_line(frame: &mut Frame, area: ratatui::layout::Rect, theme: &Theme) {
    crate::ui::component::rule::render(frame, area, theme);
}

fn input_page_item_style(theme: &Theme, focused: bool, selected: bool) -> Style {
    Style::default().fg(if selected {
        theme.coral
    } else if focused {
        theme.ok
    } else {
        theme.fg
    })
}

pub(super) fn render_input_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    session: &mut InputPageSession,
    config: &crate::config::Config,
    theme: &Theme,
) -> Option<Position> {
    match &mut session.page {
        InputPage::Settings(settings) => {
            render_settings(frame, area, settings, config, theme);
            None
        }
        InputPage::Login(login) => {
            render_login_scrolled(frame, area, login, &mut session.viewport, theme, config);
            None
        }
        InputPage::Model(model) => {
            render_model_page(
                frame,
                area,
                model,
                &session.focus,
                &mut session.viewport,
                theme,
                config,
            );
            None
        }
        InputPage::Effort(effort) => {
            render_effort_page(
                frame,
                area,
                effort,
                &session.focus,
                &mut session.viewport,
                theme,
                config,
            );
            None
        }
        InputPage::Theme(page) => {
            render_theme_page(
                frame,
                area,
                page,
                &session.focus,
                &mut session.viewport,
                theme,
                config,
            );
            None
        }
        InputPage::Resume(page) => {
            render_resume_page(frame, area, page, &mut session.viewport, theme, config);
            None
        }
        InputPage::Question(batch) => render_question_page(
            frame,
            area,
            batch,
            &session.focus,
            &mut session.viewport,
            theme,
            config,
        ),
    }
}

/// Wrap `text` into rows of at most `width` display columns using greedy
/// word wrapping. Grapheme clusters are never split; a word wider than the
/// whole row falls back to grapheme splitting.
pub use crate::wrap::wrap_text;

/// Trim `text` to at most `width` display columns, appending `…` when it
/// overflows (the ellipsis itself counts against the budget).
pub(super) fn trim_to_width(text: &str, width: usize) -> String {
    crate::wrap::ellipsize_text(text, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn model_height_tracks_active_column_and_terminal_cap() {
        let mut page = InputPageSession::model();
        assert_eq!(preferred_rows(&page), 7);
        let provider = |id: &str, count: usize| crate::agent::ModelProvider {
            id: id.into(),
            name: id.into(),
            models: (0..count)
                .map(|index| crate::agent::ModelDescriptor {
                    id: index.to_string(),
                    name: index.to_string(),
                    description: None,
                    context_window: None,
                    reasoning: None,
                })
                .collect(),
        };
        page.apply_model(vec![provider("small", 1), provider("large", 40)], None);
        assert_eq!(preferred_rows(&page), 8);
        if let InputPage::Model(model) = &mut page.page {
            model.active_provider = Some("large".into());
        }
        assert_eq!(preferred_rows(&page), 46);
        let input = InputState::new(&crate::Config::default());
        for (height, expected) in [(30, 20), (90, 46), (3, 0)] {
            assert_eq!(
                bottom_area_rows(height, 80, &input, Some(preferred_rows(&page)), 1, None),
                expected
            );
        }
        page.apply_model(Vec::new(), None);
        assert_eq!(preferred_rows(&page), 7);
    }

    #[test]
    fn model_marks_suffix_survives_name_clipping_and_tracks_mapping_changes() {
        let mut page = InputPageSession::model();
        page.apply_model(
            vec![crate::agent::ModelProvider {
                id: "p".into(),
                name: "Provider".into(),
                models: ["short", "模型 very long name repeated many times"]
                    .into_iter()
                    .map(|name| crate::agent::ModelDescriptor {
                        id: name.into(),
                        name: name.into(),
                        description: None,
                        context_window: None,
                        reasoning: None,
                    })
                    .collect(),
            }],
            Some(("p".into(), "short".into())),
        );
        let mut config = crate::Config::default();
        config.model_marks.toggle('a', "p", "short");
        config
            .model_marks
            .toggle('b', "p", "模型 very long name repeated many times");
        for width in [1, 5, 12, 32, 80, 120] {
            for blocked in [false, true] {
                config.key_mapping = crate::key_mapping::KeyMapping::from_user_toml(if blocked {
                    "[page]\nconfirm='a'\nback='shift-b'"
                } else {
                    ""
                })
                .unwrap();
                let mut terminal = Terminal::new(TestBackend::new(width, 10)).unwrap();
                terminal
                    .draw(|frame| {
                        render_input_page(frame, frame.area(), &mut page, &config, &config.theme());
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                let text: String = (0..10)
                    .flat_map(|y| (0..width).map(move |x| buffer[(x, y)].symbol()))
                    .collect();
                if width >= 12 {
                    assert_eq!(text.contains(" [a]"), !blocked, "{width}: {text}");
                    assert_eq!(text.contains(" [b]"), !blocked, "{width}: {text}");
                }
                if width >= 80 && !blocked {
                    assert!(text.contains("short [a]"), "{text}");
                    assert!(text.contains("Shift+letter mark/unmark"), "{text}");
                }
            }
        }
    }

    #[test]
    fn effort_height_includes_default_row_and_scroll_keeps_focus_visible() {
        let mut session = InputPageSession::effort();
        assert_eq!(preferred_rows(&session), 7);
        if let InputPage::Effort(page) = &mut session.page {
            page.loading = false;
            page.efforts = (0..5)
                .map(|index| crate::agent::ReasoningEffort {
                    id: index.to_string(),
                    name: format!("Effort {index}"),
                    description: None,
                })
                .collect();
        }
        session.rebuild_focus();
        session.focus.set(FocusId::new("effort:4"));
        assert_eq!(preferred_rows(&session), 12);
        let mut terminal = Terminal::new(TestBackend::new(80, 9)).unwrap();
        terminal
            .draw(|frame| {
                render_input_page(
                    frame,
                    frame.area(),
                    &mut session,
                    &crate::Config::default(),
                    &Theme::default(),
                );
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Effort 4"));
        assert_eq!(session.viewport.start, 3);
        if let InputPage::Effort(page) = &mut session.page {
            page.default_effort = Some("2".into());
        }
        assert_eq!(preferred_rows(&session), 11);
    }

    #[test]
    fn model_and_effort_keep_blank_row_above_footer() {
        let config = crate::Config::default();
        for count in [0, 1, 20] {
            let mut model = InputPageSession::model();
            model.apply_model(
                vec![crate::agent::ModelProvider {
                    id: "p".into(),
                    name: "Provider".into(),
                    models: (0..count)
                        .map(|index| crate::agent::ModelDescriptor {
                            id: index.to_string(),
                            name: format!("Model {index}"),
                            description: None,
                            context_window: None,
                            reasoning: None,
                        })
                        .collect(),
                }],
                None,
            );
            let mut effort = InputPageSession::effort();
            if let InputPage::Effort(page) = &mut effort.page {
                page.loading = false;
                page.default_effort = Some("0".into());
                page.efforts = (0..count)
                    .map(|index| crate::agent::ReasoningEffort {
                        id: index.to_string(),
                        name: format!("Effort {index}"),
                        description: None,
                    })
                    .collect();
            }
            effort.rebuild_focus();
            if count > 0 {
                model
                    .focus
                    .set(FocusId::new(format!("model:p:{}", count - 1)));
                effort
                    .focus
                    .set(FocusId::new(format!("effort:{}", count - 1)));
            }
            for mut page in [
                model,
                effort,
                InputPageSession::model(),
                InputPageSession::effort(),
            ] {
                for height in [preferred_rows(&page) as u16, 9, 6, 3] {
                    let mut terminal = Terminal::new(TestBackend::new(120, height)).unwrap();
                    terminal
                        .draw(|frame| {
                            render_input_page(
                                frame,
                                frame.area(),
                                &mut page,
                                &config,
                                &config.theme(),
                            );
                        })
                        .unwrap();
                    if height < 6 {
                        continue;
                    }
                    let buffer = terminal.backend().buffer();
                    let row = |y| {
                        (0..120)
                            .map(|x| buffer[(x, y)].symbol())
                            .collect::<String>()
                    };
                    assert!(
                        row(height - 3).trim().is_empty(),
                        "gap: {}",
                        row(height - 3)
                    );
                    assert!(row(height - 2).contains("Confirm"));
                    if count > 0 && height >= 9 {
                        let expected = match &page.page {
                            InputPage::Model(model) if !model.loading => {
                                format!("Model {}", count - 1)
                            }
                            InputPage::Effort(effort) if !effort.loading => {
                                format!("Effort {}", count - 1)
                            }
                            _ => continue,
                        };
                        assert!(
                            (3..height - 3).any(|y| row(y).contains(&expected)),
                            "{expected}"
                        );
                    }
                }
            }
        }
    }

    fn page_cases() -> Vec<(InputPageSession, &'static str, &'static str)> {
        vec![
            (
                InputPageSession::settings(crate::settings::SettingsState::default()),
                "Appearance",
                "外观",
            ),
            (InputPageSession::login(), "Login", "登录"),
            (InputPageSession::model(), "Model", "模型"),
            (InputPageSession::effort(), "Reasoning effort", "推理强度"),
            (InputPageSession::theme(&[], "ferra"), "Theme", "主题"),
            (InputPageSession::resume(), "Resume session", "续接会话"),
            (
                InputPageSession::question(crate::question::QuestionBatch::new(
                    "rpc".into(),
                    "session".into(),
                    vec![crate::agent::Question {
                        id: "question".into(),
                        question: "Keep this question verbatim".into(),
                        header: None,
                        options: None,
                        multi_select: false,
                    }],
                )),
                "Question",
                "问题",
            ),
        ]
    }

    fn rendered_page_text(mut page: InputPageSession, language: crate::Language) -> String {
        let mut config = crate::Config::default();
        config.language = language;
        let theme = config.theme();
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal
            .draw(|frame| {
                render_input_page(frame, frame.area(), &mut page, &config, &theme);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..20)
            .flat_map(|y| (0..100).map(move |x| buffer[(x, y)].symbol()))
            .collect()
    }

    #[test]
    fn input_pages_render_frontend_chrome_in_both_languages() {
        for (page, english, chinese) in page_cases() {
            let en = rendered_page_text(page, crate::Language::English);
            assert!(en.contains(english), "English page text: {en:?}");

            let (page, _, _) = page_cases()
                .into_iter()
                .find(|(_, expected_en, _)| *expected_en == english)
                .expect("page case exists");
            let zh = rendered_page_text(page, crate::Language::SimplifiedChinese);
            let compact = zh
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            assert!(compact.contains(chinese), "Chinese page text: {zh:?}");
        }
    }

    fn assert_page_keeps_values(
        page: InputPageSession,
        language: crate::Language,
        values: &[&str],
    ) {
        let text = rendered_page_text(page, language);
        let compact = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        for value in values {
            assert!(
                text.contains(value) || compact.contains(&value.replace(' ', "")),
                "{value:?} was lost from {language:?} Input Page: {text:?}"
            );
        }
    }

    #[test]
    fn free_text_question_footer_omits_option_navigation() {
        for language in crate::Language::ALL {
            let page = page_cases()
                .into_iter()
                .find(|(_, english, _)| *english == "Question")
                .expect("question page case exists")
                .0;
            let text = rendered_page_text(page, language);
            assert!(
                !text.contains("←/→"),
                "text question has arrow navigation: {text:?}"
            );
            assert!(
                !text.contains("↑/↓"),
                "text question has option navigation: {text:?}"
            );
        }
    }

    #[test]
    fn populated_input_pages_keep_external_values_in_both_languages() {
        for language in crate::Language::ALL {
            let mut login = InputPageSession::login();
            if let InputPage::Login(page) = &mut login.page {
                page.page = crate::login::Page::Providers;
                page.loading = false;
                page.providers = vec![crate::agent::CredentialProvider {
                    id: "provider-id".into(),
                    name: "Provider from host".into(),
                    api_key_configured: true,
                    api_key_writable: true,
                    api_key_source: Some("environment".into()),
                    api_key_hint: Some("…1234".into()),
                }];
            }
            login.rebuild_focus();
            assert_page_keeps_values(login, language, &["Provider from host", "…1234"]);

            let mut model = InputPageSession::model();
            model.apply_model(
                vec![crate::agent::ModelProvider {
                    id: "provider-id".into(),
                    name: "Provider from host".into(),
                    models: vec![crate::agent::ModelDescriptor {
                        id: "model-id".into(),
                        name: "Model from host".into(),
                        description: Some("Model description from host".into()),
                        context_window: None,
                        reasoning: None,
                    }],
                }],
                Some(("provider-id".into(), "model-id".into())),
            );
            assert_page_keeps_values(model, language, &["Provider from host", "Model from host"]);

            let mut effort = InputPageSession::effort();
            effort.apply_effort(&crate::CatalogModel {
                current_model: Some(crate::agent::ModelSelection {
                    provider: "provider-id".into(),
                    model: "model-id".into(),
                    reasoning_effort: Some("host-effort".into()),
                }),
                model_providers: vec![crate::agent::ModelProvider {
                    id: "provider-id".into(),
                    name: "Provider from host".into(),
                    models: vec![crate::agent::ModelDescriptor {
                        id: "model-id".into(),
                        name: "Model from host".into(),
                        description: None,
                        context_window: None,
                        reasoning: Some(crate::agent::ModelReasoning {
                            efforts: vec![crate::agent::ReasoningEffort {
                                id: "host-effort".into(),
                                name: "Effort from host".into(),
                                description: None,
                            }],
                            default_effort: None,
                        }),
                    }],
                }],
                ..Default::default()
            });
            assert_page_keeps_values(effort, language, &["Effort from host"]);

            let mut resume = InputPageSession::resume();
            resume.apply_sessions(
                vec![crate::agent::SessionSummary {
                    id: "session-id".into(),
                    title: "Session title from host".into(),
                    live: false,
                    created_at: 1,
                }],
                false,
            );
            assert_page_keeps_values(resume, language, &["Session title from host", "session-id"]);

            let question = InputPageSession::question(crate::question::QuestionBatch::new(
                "rpc".into(),
                "session".into(),
                vec![crate::agent::Question {
                    id: "question-id".into(),
                    question: "Question text from host".into(),
                    header: Some("Question header from host".into()),
                    options: Some(vec![crate::agent::QuestionOption {
                        label: "Option from host".into(),
                        description: Some("Option description from host".into()),
                    }]),
                    multi_select: false,
                }],
            ));
            assert_page_keeps_values(
                question,
                language,
                &[
                    "Question header from host",
                    "Question text from host",
                    "Option from host",
                ],
            );

            let mut settings =
                InputPageSession::settings(crate::settings::SettingsState::default());
            if let InputPage::Settings(page) = &mut settings.page {
                page.category = 1;
                page.pos[1] = 1;
            }
            settings.rebuild_focus();
            let language_label = if language == crate::Language::English {
                "Language"
            } else {
                "语言"
            };
            assert_page_keeps_values(settings, language, &[language_label]);
        }
    }

    #[test]
    fn wrap_text_splits_at_exact_display_width() {
        assert_eq!(wrap_text("abcdef", 3), vec!["abc", "def"]);
        assert_eq!(wrap_text("abc", 3), vec!["abc"]);
        assert_eq!(wrap_text("", 3), Vec::<String>::new());
        // CJK double-width glyphs count as two columns.
        assert_eq!(wrap_text("你好世界", 4), vec!["你好", "世界"]);
    }

    #[test]
    fn wrap_text_keeps_zwj_and_combining_graphemes_intact() {
        // Family emoji is one grapheme cluster of 2 display columns; a narrow
        // box must keep it whole instead of splitting mid-cluster at a char
        // boundary.
        let family = "👨‍👩‍👧";
        assert_eq!(wrap_text(family, 2), vec![family.to_string()]);
        assert_eq!(
            wrap_text(&format!("{family}{family}"), 3),
            vec![family.to_string(); 2],
            "two clusters split cleanly at the cluster boundary"
        );
        // A combining mark stays glued to its base char.
        assert_eq!(
            wrap_text("e\u{301}x", 1),
            vec!["e\u{301}".to_string(), "x".to_string()]
        );
    }
}
