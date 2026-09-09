use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    layout::Position,
    style::{Color, Modifier},
    Terminal,
};

use crate::{
    app::TuiApp,
    display::{CardRole, ContentCard, DisplayId, DisplayItem, DisplayTone},
    input::InputState,
    mouse_selection::MouseSelection,
    preview::{
        PreviewContent, PreviewState, ToolMetrics, ToolPreview, ToolPreviewPrimary,
        ToolPreviewSecondary,
    },
    theme::Theme,
    PointerEvent,
};

use super::*;

fn overlays() -> RenderOverlays<'static> {
    RenderOverlays {
        help_visible: false,
        toast: None,
        input_page: None,
        settings: None,
        login: None,
        approval: None,
        queue: &[],
        pane_resize: Default::default(),
    }
}

/// Layout inputs of the composer text area for a terminal of `width`
/// columns, derived from the live pane configuration instead of a hardcoded
/// default percentage: the page starts three columns in (page margin,
/// prompt, and input gutter) and the editable text spans `page_width - 3`
/// columns.
fn composer_text_area(state: &TuiApp, width: u16) -> (usize, usize) {
    let page = usize::from(crate::ui::input_bar_width(width, state));
    let inner = page.saturating_sub(3);
    (3, inner)
}

/// Column of the pane separator for a terminal of `width` columns under the
/// live message-pane percentage (no persisted default baked in).
fn separator_column(state: &TuiApp, width: u16) -> usize {
    usize::from(state.config.message_pane_percent.columns(width))
}

/// Vertical geometry of the input box for `content` chars inside an
/// `inner`-wide text area. The bottom stack below the box is one gap row,
/// one status row, and one title row.
struct InputBoxGeometry {
    box_top: u16,
    last_y: u16,
}

fn input_box_geometry(
    terminal_height: u16,
    content: usize,
    extra_rows: usize,
    inner: usize,
) -> InputBoxGeometry {
    let full_rows = content / inner;
    let tail = content - full_rows * inner;
    let rows = (extra_rows + full_rows + usize::from(tail > 0)).min(5);
    let box_top = terminal_height - 3 - (rows as u16 + 2);
    InputBoxGeometry {
        box_top,
        last_y: box_top + rows as u16,
    }
}

fn force_message_only(state: &mut TuiApp) {
    state.config.message_pane_percent = crate::PaneWidthPercent::from_basis_points(10_000).unwrap();
}

fn force_preview_only(state: &mut TuiApp) {
    force_message_only(state);
    state.preview.fullscreen = true;
}

fn find_text(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let area = *buffer.area();
    let width = needle.chars().count() as u16;
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width).saturating_sub(width) {
            if needle
                .chars()
                .enumerate()
                .all(|(offset, ch)| buffer[(x + offset as u16, y)].symbol() == ch.to_string())
            {
                return Some((x, y));
            }
        }
    }
    None
}

fn row_slice(row: &str, start: usize, width: usize) -> String {
    row.chars().skip(start).take(width).collect()
}

#[test]
fn pending_submissions_render_before_any_agent_echo() {
    for drafting in [false, true] {
        for (prompt, visible) in [
            ("submitted text", "submitted text"),
            ("/skill:review", "[Skill] review"),
        ] {
            let mut state = crate::runtime::RuntimeState::default();
            force_message_only(&mut state.tui);
            if drafting {
                state.push_system_message("old session content");
                state.begin_new_conversation("standard");
                state.materialize_new_conversation(crate::PromptInput::text(prompt));
            } else {
                state.admit_submission(&crate::PromptInput::text(prompt), true);
            }
            let input = InputState::new(&state.config);
            let theme = state.theme();
            let mut scroll = ScrollState::default();
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| {
                    render_with_cursor(
                        frame,
                        &mut state.tui,
                        &input,
                        &mut scroll,
                        &theme,
                        overlays(),
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let (_, card_y) =
                find_text(buffer, visible).expect("submission must be visible before echo");
            if drafting {
                assert!(find_text(buffer, "old session content").is_none());
            } else {
                let (_, thinking_y) = find_text(buffer, "Thinking").expect("pending thinking");
                assert!(thinking_y > card_y);
            }
        }
    }
}

#[test]
fn help_overlay_advertises_application_paste_shortcut() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| {
            render_with_cursor(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                RenderOverlays {
                    help_visible: true,
                    ..overlays()
                },
            );
        })
        .unwrap();
    assert!(find_text(terminal.backend().buffer(), "Ctrl+V").is_some());
}

#[test]
fn local_help_markdown_renders_as_styled_transcript_content() {
    let mut runtime = crate::runtime::RuntimeState::default();
    runtime.config.resolved_theme = Theme::ferra();
    runtime.push_local_markdown(crate::help::markdown(
        &runtime.config,
        &[crate::agent::CommandDescriptor {
            name: "feedback".into(),
            description: "record feedback".into(),
            input_hint: Some("<text>".into()),
        }],
    ));
    let mut state = runtime.tui;
    force_message_only(&mut state);
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 260)).unwrap();

    terminal
        .draw(|frame| {
            render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let heading = find_text(buffer, "e").expect("Markdown heading is visible");
    assert_eq!(buffer[heading].fg, theme.markdown.heading1.fg);
    assert!(buffer[heading].modifier.contains(Modifier::BOLD));
    let command = find_text(buffer, "/settings").expect("built-in command is visible");
    assert_eq!(buffer[command].fg, theme.markdown.inline_code.fg);
    assert!(find_text(buffer, "/feedback").is_some());
}

#[test]
fn copy_toast_is_a_popup_and_does_not_replace_the_input_draft() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let mut input = InputState::new(&state.config);
    input.buf = "draft text".into();
    input.cursor = input.buf.chars().count();
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                RenderOverlays {
                    toast: Some("已复制 2 行：测试内容..."),
                    ..overlays()
                },
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(find_text(buffer, "draft text").is_some());
    assert!(anchor.is_some(), "toast must not hide the composer cursor");
    let toast_y = (0..20u16)
        .find(|&y| (0..80u16).any(|x| buffer[(x, y)].symbol() == "已"))
        .expect("copy toast renders");
    assert!(toast_y <= 2, "toast is a top popup, not input content");
    let compact = (0..80u16)
        .map(|x| buffer[(x, toast_y)].symbol())
        .collect::<String>()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    assert!(compact.contains("✓已复制2行：测试内容..."));
    assert!((0..80u16).any(|x| buffer[(x, toast_y - 1)].symbol() == "─"));
}

#[test]
fn input_bar_paste_block_renders_placeholder_between_editable_text() {
    // Typed text + an over-threshold paste + typed text: the paste collapses
    // into one Rose placeholder span while the surrounding text keeps the
    // Mist text tone, and the cursor (after the trailing text) stays on the
    // same single row.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let mut input = InputState::new(&state.config);
    input.paste_placeholder_chars = 5;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let plain = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    for c in "ab".chars() {
        input.handle_key(&plain(c), true);
    }
    input.paste("123456");
    for c in "cd".chars() {
        input.handle_key(&plain(c), true);
    }
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y: u16| {
        (0..80)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    // Find the single text row that carries the projected content.
    let expected = "ab[6 text pasted]cd";
    let text_row = (15u16..22)
        .find(|&y| row(y).contains(expected))
        .unwrap_or_else(|| {
            panic!(
                "placeholder row not found; rows: {:?}",
                (15u16..22).map(row).collect::<Vec<_>>()
            )
        });
    let start_x = find_text(buffer, expected)
        .expect("placeholder content is visible")
        .0 as usize;
    // Placeholder characters use the Rose placeholder tone; typed text Mist.
    let placeholder_fg = theme.input.placeholder.fg;
    let text_fg = theme.input.text.fg;
    let placeholder_span = 2..2 + "[6 text pasted]".chars().count();
    for i in 0..expected.chars().count() {
        let x = (start_x + i) as u16;
        let cell = &buffer[(x, text_row)];
        let expected_fg = if placeholder_span.contains(&i) {
            placeholder_fg
        } else {
            text_fg
        };
        assert_eq!(cell.fg, expected_fg, "char {i} of {expected:?}");
    }
    // The IME anchor sits right after the trailing text on the same row.
    assert_eq!(
        anchor,
        Some(Position::new(
            (start_x + expected.chars().count()) as u16,
            text_row
        )),
        "IME anchor sits right after the trailing text"
    );
}

#[test]
fn input_bar_box_grows_with_wrapped_rows_and_keeps_cursor_visible() {
    // 200 chars wrap onto ceil(200 / inner) rows; the box is capped at 5 text
    // rows and the wrap window follows the cursor, so the last five wrapped
    // rows stay visible and the IME anchor stays inside the text area. The
    // text geometry comes from the live pane configuration.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let (text_x, inner) = composer_text_area(&state, 80);
    let mut input = InputState::new(&state.config);
    input.buf = "x".repeat(200);
    input.cursor = input.buf.chars().count();
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y: u16| {
        (0..80u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    // The box grows with the wrapped content (capped at 5 text rows) and the
    // wrap window follows the cursor, so the last five wrapped rows stay
    // visible and the IME anchor stays inside the text area.
    let tail = 200 % inner;
    let geo = input_box_geometry(24, 200, 0, inner);
    let box_top = geo.box_top;
    let last_y = geo.last_y;
    for offset in 0..(200 / inner).min(4) {
        let y = box_top + 1 + offset as u16;
        assert_eq!(
            row_slice(&row(y), text_x, inner),
            "x".repeat(inner),
            "full text row {y}"
        );
    }
    assert_eq!(
        row_slice(&row(last_y), text_x, tail),
        "x".repeat(tail),
        "cursor row shows the tail chunk"
    );
    assert!(
        row(last_y + 1)
            .trim()
            .chars()
            .all(|character| character == '─'),
        "bottom rule of the composer"
    );
    assert_eq!(
        anchor,
        Some(Position::new((text_x + tail) as u16, last_y)),
        "IME anchor sits on the cursor row inside the text area"
    );
    assert_eq!(
        buffer[((text_x + tail) as u16, last_y)].fg,
        theme
            .input
            .cursor
            .bg
            .expect("ferra cursor has a fill color"),
        "drawn cursor on the wrapped cursor row"
    );
    assert_eq!(buffer[((text_x + tail) as u16, last_y)].bg, Color::Reset);
}

#[test]
fn input_bar_full_final_row_keeps_cursor_out_of_bottom_padding() {
    // 180 chars fill full rows plus a short tail at the live inner width. The
    // synthetic cursor space after the final character must stay on the last
    // text row (in the box's right gutter), not wrap into the bottom padding.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let (text_x, inner) = composer_text_area(&state, 80);
    let mut input = InputState::new(&state.config);
    input.buf = "x".repeat(180);
    input.cursor = input.buf.chars().count();
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y: u16| {
        (0..80u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    let tail = 180 % inner;
    let geo = input_box_geometry(24, 180, 0, inner);
    let box_top = geo.box_top;
    let last_y = geo.last_y;
    for offset in 0..180 / inner {
        let y = box_top + 1 + offset as u16;
        assert_eq!(
            row_slice(&row(y), text_x, inner),
            "x".repeat(inner),
            "full text row {y}"
        );
    }
    assert_eq!(
        row_slice(&row(last_y), text_x, tail),
        "x".repeat(tail),
        "tail row stays inside the text area"
    );
    assert!(
        row(last_y + 1)
            .trim()
            .chars()
            .all(|character| character == '─'),
        "bottom rule stays intact"
    );
    assert_eq!(anchor, Some(Position::new((text_x + tail) as u16, last_y)));
    assert_eq!(
        buffer[((text_x + tail) as u16, last_y)].fg,
        theme
            .input
            .cursor
            .bg
            .expect("ferra cursor has a fill color"),
        "cursor block is patched into the right gutter on the last text row"
    );
    assert_eq!(buffer[((text_x + tail) as u16, last_y)].bg, Color::Reset);
}

#[test]
fn input_bar_multiline_wrapped_rows_keep_cursor_row_in_box() {
    // "a\n" + 200 y's: several display rows, box capped at 5 text rows; the
    // window follows the cursor so the last five wrapped rows are shown.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let (text_x, inner) = composer_text_area(&state, 80);
    let mut input = InputState::new(&state.config);
    input.multiline = true;
    input.buf = format!("a\n{}", "y".repeat(200));
    input.cursor = input.buf.chars().count();
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y: u16| {
        (0..80u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    let tail = 200 % inner;
    let geo = input_box_geometry(24, 200, 1, inner);
    let box_top = geo.box_top;
    let last_y = geo.last_y;
    for offset in 0..4 {
        let y = box_top + 1 + offset as u16;
        assert_eq!(
            row_slice(&row(y), text_x, inner),
            "y".repeat(inner),
            "visible row {y}"
        );
    }
    assert_eq!(
        row_slice(&row(last_y), text_x, tail),
        "y".repeat(tail),
        "cursor row shows the tail chunk"
    );
    assert_eq!(
        anchor,
        Some(Position::new((text_x + tail) as u16, last_y)),
        "IME anchor stays on the cursor row inside the text area"
    );
}

#[test]
fn input_bar_fits_all_wrapped_rows_when_they_fit_the_box() {
    // 100 chars -> wrapped rows at the live inner width -> the content still
    // fits inside the 5-row cap, so the whole content is visible from the top;
    // the cursor row is the last one.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let (text_x, inner) = composer_text_area(&state, 80);
    let mut input = InputState::new(&state.config);
    input.buf = "z".repeat(100);
    input.cursor = input.buf.chars().count();
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y: u16| {
        (0..80u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    let tail = 100 % inner;
    assert_eq!(100 / inner, 2, "100 chars occupy two full rows plus a tail");
    let geo = input_box_geometry(24, 100, 0, inner);
    let box_top = geo.box_top;
    let last_y = geo.last_y;
    assert_eq!(
        row_slice(&row(box_top + 1), text_x, inner),
        "z".repeat(inner),
        "row 1 from the top"
    );
    assert_eq!(
        row_slice(&row(box_top + 2), text_x, inner),
        "z".repeat(inner),
        "row 2 from the top"
    );
    assert_eq!(
        row_slice(&row(last_y), text_x, tail),
        "z".repeat(tail),
        "row 3 from the top"
    );
    assert_eq!(anchor, Some(Position::new((text_x + tail) as u16, last_y)));
}

#[test]
fn input_box_rows_follow_wrapped_content() {
    let config = crate::config::Config::default();
    let mut input = InputState::new(&config);
    // 40-column bar with 2-column gutters and one prompt column → inner 35.
    assert_eq!(input_rows(&input, 40, 2, None), 1, "empty input is one row");
    input.buf = "x".repeat(35);
    assert_eq!(input_rows(&input, 40, 2, None), 1, "exactly one row fits");
    input.buf = "x".repeat(36);
    assert_eq!(
        input_rows(&input, 40, 2, None),
        2,
        "one overflow column wraps"
    );
    input.buf = "x".repeat(200);
    assert_eq!(
        input_rows(&input, 40, 2, None),
        INPUT_MAX_ROWS,
        "cap at 5 rows"
    );
    // Trailing newline still counts as an extra row (Shift+Enter growth).
    input.buf = "a\n".into();
    assert_eq!(input_rows(&input, 40, 2, None), 2);
    // A single long line wraps inside a narrow bar and grows the box.
    input.buf = "a very long single line that wraps".into();
    let rows = input_rows(&input, 16, 0, None);
    assert!((2..=INPUT_MAX_ROWS).contains(&rows));
}

#[test]
fn input_bar_word_wrap_keeps_cursor_anchored_after_consumed_space() {
    // 9 `aaaa` words fit in the 44-column input content width; the following
    // separator is consumed by the greedy break, so the cursor sitting on
    // that separator must render at the end of the previous wrapped row.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let mut input = InputState::new(&state.config);
    input.buf = "aaaa ".repeat(10);
    input.cursor = 34; // the consumed space between row 1 and row 2
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y: u16| {
        (0..80u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    assert_eq!(
        row_slice(&row(18), 3, 44),
        "aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa",
        "first wrapped row keeps the nine fitting words"
    );
    assert_eq!(
        row_slice(&row(19), 3, 4),
        "aaaa",
        "second wrapped row starts with the next whole word"
    );
    assert_eq!(
        anchor,
        Some(Position::new(37, 18)),
        "IME anchor sits after the first wrapped row at the consumed space"
    );
}

#[test]
fn extracted_main_pane_preserves_status_spacing_and_hidden_cursor() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.session.session_id = Some("session".into());
    state.session.session_title = Some("refactor bridge".into());
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let backend = TestBackend::new(80, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut anchor = None;
    terminal
        .draw(|frame| {
            anchor = render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();

    assert_eq!(anchor, Some(Position::new(3, 35)));
    assert_eq!(terminal.get_cursor_position().unwrap(), Position::new(0, 0));
    let buffer = terminal.backend().buffer();
    let row = |y| {
        (0..80)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    let main_row = |y| row(y).chars().take(48).collect::<String>();
    assert!(main_row(37).trim().is_empty());
    assert!(
        main_row(38).contains("e·dsh standard"),
        "status row: {:?}",
        main_row(38)
    );
    assert!(row(38).contains("Ctrl+H Help"));
    assert!(main_row(39).contains("refactor bridge"));
    assert_eq!(buffer[(1, 38)].bg, Color::Reset);
}

#[test]
fn activity_rows_use_braille_while_running_and_bullets_when_settled() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);

    let running =
        crate::display::ActivityRow::root(DisplayId::correlated("indicator", "running"), "running");
    let mut success =
        crate::display::ActivityRow::root(DisplayId::correlated("indicator", "success"), "success");
    success.state = crate::display::ActivityState::Success;
    let mut failure =
        crate::display::ActivityRow::root(DisplayId::correlated("indicator", "failure"), "failure");
    failure.state = crate::display::ActivityState::Failure;
    for row in [running, success, failure] {
        state.transcript.append(DisplayItem::Activity(row), None);
    }

    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("⠋ running"));
    assert!(rendered.contains("• success"));
    assert!(rendered.contains("• failure"));
}

#[test]
fn long_activity_run_folds_in_normal_mode_and_expands_in_reading_view() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    for index in 0..8 {
        state.transcript.append(
            DisplayItem::Activity(crate::display::ActivityRow::root(
                DisplayId::correlated("activity-fold", &index.to_string()),
                format!("tool-{index}"),
            )),
            None,
        );
    }
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant-answer", "fold-boundary"),
            unit: None,
            content: "done".into(),
            format: crate::display::TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: "done".into(),
            streaming: false,
        }),
        None,
    );
    let mut input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let normal = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(normal.contains("... (2 lines)"));
    assert!(!normal.contains("tool-3"));
    assert!(!normal.contains("tool-4"));

    assert!(state.enter_reading(&input, &mut scroll, 16));
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let reading = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!reading.contains("... (2 lines)"));
    assert!(reading.contains("tool-3"));
    assert!(reading.contains("tool-4"));

    state.exit_reading(&mut input);
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let normal_again = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(normal_again.contains("... (2 lines)"));
}

#[test]
fn entering_reading_keeps_the_selected_block_anchored_when_a_fold_expands() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    for index in 0..20 {
        state.transcript.append(
            DisplayItem::Activity(crate::display::ActivityRow::root(
                DisplayId::correlated("activity-anchor", &index.to_string()),
                format!("tool-{index}"),
            )),
            None,
        );
    }
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant-answer", "anchor"),
            unit: None,
            content: "done".into(),
            format: crate::display::TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: "done".into(),
            streaming: false,
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    scroll.follow = false;
    scroll.offset = 4;
    assert!(state.enter_reading(&input, &mut scroll, 6));
    let selected = state.reading.as_ref().unwrap().block_cursor.clone();
    let folded_row = state.reading_layout.block(&selected).unwrap().rows.start;
    let screen_row = folded_row.saturating_sub(scroll.offset);

    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    let expanded_row = state.reading_layout.block(&selected).unwrap().rows.start;
    assert!(
        expanded_row > folded_row,
        "the selected block moves after expansion"
    );
    assert_eq!(
        expanded_row.saturating_sub(scroll.offset),
        screen_row,
        "Reading entry preserves the selected block's viewport anchor"
    );
}

#[test]
fn streaming_tail_splice_replaces_the_whole_growing_markdown_suffix() {
    fn streaming_block(id: DisplayId, content: &str) -> DisplayItem {
        DisplayItem::Block(crate::display::TranscriptBlock {
            id,
            unit: None,
            content: content.into(),
            format: crate::display::TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: content.into(),
            streaming: true,
        })
    }

    fn draw(state: &mut TuiApp) {
        let input = InputState::new(&state.config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
        terminal
            .draw(|frame| render(frame, state, &input, &mut scroll, &theme, overlays()))
            .unwrap();
    }

    fn replace_source(state: &mut TuiApp, id: &DisplayId, content: &str) {
        let node = state.transcript.get_mut(id).expect("streaming block");
        let DisplayItem::Block(block) = &mut node.item else {
            panic!("expected markdown block");
        };
        block.content = content.into();
        block.copy_source = content.into();
        state.transcript.touch(id);
        state.render.transcript_cache.mark_tail_dirty();
    }

    let id = DisplayId::correlated("assistant", "growing-tail");
    let mut incremental = TuiApp::default();
    incremental.config.resolved_theme = Theme::ferra();
    incremental
        .transcript
        .append(streaming_block(id.clone(), "alpha"), None);
    draw(&mut incremental);
    replace_source(&mut incremental, &id, "alpha\n\nbeta");
    draw(&mut incremental);
    replace_source(&mut incremental, &id, "alpha\n\nbeta\n\ngamma");
    draw(&mut incremental);

    let final_source = "alpha\n\nbeta\n\ngamma";
    let mut rebuilt = TuiApp::default();
    rebuilt.config.resolved_theme = Theme::ferra();
    rebuilt
        .transcript
        .append(streaming_block(id, final_source), None);
    draw(&mut rebuilt);

    let incremental_lines = incremental
        .render
        .transcript_cache
        .lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>();
    let rebuilt_lines = rebuilt
        .render
        .transcript_cache
        .lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        incremental_lines, rebuilt_lines,
        "incremental streaming must not retain rows from older tail renders"
    );
    assert_eq!(
        incremental_lines
            .iter()
            .filter(|line| line.contains("alpha"))
            .count(),
        1,
        "the first streamed paragraph must not be duplicated"
    );
}

#[test]
fn paced_markdown_reveal_splices_a_non_tail_suffix_and_keeps_full_source() {
    fn block(id: &str, content: &str, format: crate::display::TranscriptFormat) -> DisplayItem {
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("reveal-test", id),
            unit: None,
            content: content.into(),
            format,
            tone: DisplayTone::Normal,
            copy_source: content.into(),
            streaming: format == crate::display::TranscriptFormat::Markdown,
        })
    }

    fn draw(state: &mut TuiApp) {
        let input = InputState::new(&state.config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
        terminal
            .draw(|frame| render(frame, state, &input, &mut scroll, &theme, overlays()))
            .unwrap();
    }

    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.config.message_chars_per_second = crate::config::RevealRate::new(16).unwrap();
    state.transcript.append(
        block("before", "before", crate::display::TranscriptFormat::Plain),
        None,
    );
    let reveal_id = DisplayId::correlated("reveal-test", "answer");
    state.transcript.append(
        block(
            "answer",
            "abcdef",
            crate::display::TranscriptFormat::Markdown,
        ),
        None,
    );
    state.transcript.append(
        block("after", "after", crate::display::TranscriptFormat::Plain),
        None,
    );
    state
        .render
        .transcript_reveals
        .insert(reveal_id.clone(), crate::reveal::RevealTrack::default());

    draw(&mut state);
    let first = state
        .render
        .transcript_cache
        .lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>();
    assert!(
        !first.iter().any(|line| line.contains("abcdef")),
        "an open trailing word waits for stable admission"
    );
    let node = state.transcript.get(&reveal_id).unwrap();
    assert!(matches!(
        &node.item,
        DisplayItem::Block(block) if block.copy_source == "abcdef" && block.content == "abcdef"
    ));
    assert!(state.render.units.values().any(|source| source == "abcdef"));

    state.render.transcript_cache.take_work_stats();
    let admission_due = state
        .transcript_reveal_deadline()
        .expect("held streaming tail has a timeout");
    assert!(state.tick_transcript_reveals(admission_due));
    draw(&mut state);
    let admitted = state
        .render
        .transcript_cache
        .lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>();
    assert!(admitted.iter().any(|line| line.contains("a ⠋")));

    let fade_due = state.transcript_reveal_deadline().expect("first fade");
    assert!(state.tick_transcript_reveals(fade_due));
    let final_fade_due = state.transcript_reveal_deadline().expect("final fade");
    assert!(state.tick_transcript_reveals(final_fade_due));
    let due = state.transcript_reveal_deadline().expect("queued reveal");
    assert!(state.tick_transcript_reveals(due));
    draw(&mut state);
    let second = state
        .render
        .transcript_cache
        .lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>();
    assert!(second.iter().any(|line| line.contains("ab ⠋")));
    assert!(second.iter().any(|line| line.contains("before")));
    assert!(second.iter().any(|line| line.contains("after")));
    let work = state.render.transcript_cache.take_work_stats();
    assert_eq!(work.rebuilds, 0, "reveal splices from the active message");
}

#[test]
fn code_block_fill_padding_does_not_change_reveal_work_on_resize() {
    fn reveal_steps(width: u16) -> usize {
        let mut state = TuiApp::default();
        state.config.resolved_theme = Theme::ferra();
        state.config.message_chars_per_second = crate::config::RevealRate::new(16).unwrap();
        let id = DisplayId::correlated("reveal-test", &format!("code-{width}"));
        let source = "```\nx\n```";
        state.transcript.append(
            DisplayItem::Block(crate::display::TranscriptBlock {
                id: id.clone(),
                unit: None,
                content: source.into(),
                format: crate::display::TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: source.into(),
                streaming: false,
            }),
            None,
        );
        state
            .render
            .transcript_reveals
            .insert(id, crate::reveal::RevealTrack::default());

        let input = InputState::new(&state.config);
        let mut scroll = ScrollState::default();
        let theme = Theme::ferra();
        let mut terminal = Terminal::new(TestBackend::new(width, 16)).unwrap();
        terminal
            .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
            .unwrap();

        let mut steps = 0;
        while let Some(due) = state.transcript_reveal_deadline() {
            assert!(state.tick_transcript_reveals(due));
            steps += 1;
            assert!(steps < 512, "reveal work did not converge");
        }
        steps
    }

    assert_eq!(
        reveal_steps(40),
        reveal_steps(120),
        "width-only code-block fill must not change logical reveal work"
    );
}

#[test]
fn extracted_main_pane_uses_arrowless_ruled_user_style_and_keeps_copy_provenance() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let source = "wrapped user content that remains verbatim".to_string();
    state.render.units.insert(7, source.clone());
    state.transcript.append(
        DisplayItem::Card(ContentCard {
            id: DisplayId::correlated("user", "fixture"),
            unit: Some(7),
            header: None,
            content: source.clone(),
            role: CardRole::User,
            tone: DisplayTone::Normal,
            horizontal_padding: 2,
            copy_source: source.clone(),
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let backend = TestBackend::new(40, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let (content_x, content_y) = find_text(buffer, "wrapped").expect("user message is visible");
    assert_eq!(content_x, 4, "card padding plus the omitted prompt slot");
    assert_eq!(buffer[(1, content_y)].symbol(), " ", "no prompt arrow");
    assert_eq!(buffer[(content_x, content_y)].fg, theme.input.text.fg);
    assert_eq!(buffer[(content_x, content_y)].bg, Color::Reset);
    for y in [content_y - 1, content_y + 2] {
        assert!((1..37).all(|x| buffer[(x, y)].symbol() == "─"));
        assert_eq!(buffer[(1, y)].fg, theme.diff.separator.fg);
        assert_eq!(buffer[(3, y)].fg, theme.input.hint.fg);
        assert_eq!(buffer[(36, y)].fg, theme.diff.separator.fg);
    }
    assert_eq!(state.render.units.get(&7), Some(&source));
    let rows = provenance_layout_rows(&state);
    assert!(rows.iter().any(|row| row.unit == 7));
}

#[test]
fn skill_invocation_renders_compact_identity_and_keeps_full_copy_source() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let source = "expanded skill instructions that stay hidden".to_string();
    state.render.units.insert(7, source.clone());
    state.transcript.append(
        DisplayItem::Card(ContentCard {
            id: DisplayId::correlated("skill", "fixture"),
            unit: Some(7),
            header: None,
            content: "code-review".into(),
            role: CardRole::Skill,
            tone: DisplayTone::Normal,
            horizontal_padding: 2,
            copy_source: source.clone(),
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("[Skill] code-review"));
    assert!(!rendered.contains("expanded skill instructions"));
    assert_eq!(state.render.units.get(&7), Some(&source));
}

#[test]
fn context_injection_renders_as_plain_text_capped_at_two_lines() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let source = "x".repeat(200);
    state.render.units.insert(7, source.clone());
    state.transcript.append(
        DisplayItem::Card(ContentCard {
            id: DisplayId::correlated("context", "fixture"),
            unit: Some(7),
            header: Some("Context · instructions".into()),
            content: source.clone(),
            role: CardRole::Context,
            tone: DisplayTone::Dim,
            horizontal_padding: 2,
            copy_source: source.clone(),
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| {
            render(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let label = |x: u16| buffer[(x, 0)].symbol().chars().next().unwrap_or(' ');
    let row_text = |y: u16| {
        (0..40u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    // Plain text (no card shell background): the first row starts with the
    // `Prompt injection` label in the activity label tone (umber in ferra)
    // and the content in the activity detail tone (bark), with no background
    // fill.
    assert_eq!(label(1), 'P', "label glyph starts the first row");
    assert_eq!(label(2), 'r', "label glyph on the first row");
    assert_eq!(label(3), 'o', "label glyph on the first row");
    assert_eq!(label(4), 'm', "label glyph on the first row");
    assert_eq!(label(5), 'p', "label glyph on the first row");
    assert_eq!(
        buffer[(1, 0)].fg,
        theme.activity.label.fg,
        "label uses the activity label tone (umber)"
    );
    assert_eq!(
        buffer[(18, 0)].fg,
        theme.activity.detail.fg,
        "content uses the activity detail tone (bark)"
    );
    assert_eq!(buffer[(1, 0)].bg, Color::Reset, "no card background");
    assert_eq!(buffer[(18, 0)].bg, Color::Reset, "no card background");
    // Capped at two wrapped rows with an explicit ellipsis marker on row 2.
    assert_eq!(
        row_text(1).trim(),
        format!("{}…", "x".repeat(35)),
        "second row ends with the ellipsis marker"
    );
    assert!(row_text(2).trim().is_empty(), "gap row after the message");
    // Copy provenance keeps the full original text.
    assert_eq!(state.render.units.get(&7), Some(&source));
    let rows = provenance_layout_rows(&state);
    assert!(rows.iter().any(|row| row.unit == 7));
}

#[test]
fn context_injection_short_content_fits_on_one_row() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let source = "short context".to_string();
    state.transcript.append(
        DisplayItem::Card(ContentCard {
            id: DisplayId::correlated("context", "short"),
            unit: None,
            header: Some("Context · instructions".into()),
            content: source.clone(),
            role: CardRole::Context,
            tone: DisplayTone::Dim,
            horizontal_padding: 2,
            copy_source: source.clone(),
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| {
            render(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row_text = |y: u16| {
        (0..40u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    // The localized label and the plain-ASCII content share one row.
    assert_eq!(buffer[(1, 0)].symbol(), "P");
    assert_eq!(buffer[(2, 0)].symbol(), "r");
    assert_eq!(buffer[(3, 0)].symbol(), "o");
    assert_eq!(buffer[(4, 0)].symbol(), "m");
    assert_eq!(buffer[(5, 0)].symbol(), "p");
    assert!(row_text(0).contains("short context"));
    assert!(row_text(1).trim().is_empty(), "one row then the gap");
    assert_eq!(buffer[(1, 0)].fg, theme.activity.label.fg);
    assert_eq!(buffer[(18, 0)].fg, theme.activity.detail.fg);
    assert_eq!(buffer[(1, 0)].bg, Color::Reset, "no card background");
}

#[test]
fn chinese_transcript_context_label_preserves_injected_content() {
    let mut state = TuiApp::default();
    state.config.language = crate::Language::SimplifiedChinese;
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let source = "short context".to_string();
    state.transcript.append(
        DisplayItem::Card(ContentCard {
            id: DisplayId::correlated("context", "localized"),
            unit: None,
            header: Some("Context · instructions".into()),
            content: source.clone(),
            role: CardRole::Context,
            tone: DisplayTone::Dim,
            horizontal_padding: 2,
            copy_source: source.clone(),
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    let flat = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert!(
        flat.contains("提示词注入"),
        "context label is not localized: {flat}"
    );
    assert!(
        flat.contains("shortcontext"),
        "injected content changed: {flat}"
    );
}

#[test]
fn chinese_transcript_localizes_tool_metadata() {
    let mut state = TuiApp::default();
    state.config.language = crate::Language::SimplifiedChinese;
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let mut activity =
        crate::display::ActivityRow::root(DisplayId::correlated("tool", "localized"), "bash");
    activity.output_lines = Some(2);
    activity.duration_ms = Some(1200);
    state
        .transcript
        .append(DisplayItem::Activity(activity), None);
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    let flat = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert!(flat.contains("bash"));
    assert!(
        flat.contains("2行"),
        "line metadata is not localized: {flat}"
    );
    assert!(
        flat.contains("·1.2s") && !flat.contains("耗时"),
        "duration should render without a label: {flat}"
    );
}

#[test]
fn wide_screen_renders_preview_without_changing_main_provenance() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let source = "# Preview source\n\ncomplete body".to_string();
    state.render.units.insert(11, source.clone());
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "preview"),
            unit: Some(11),
            content: source.clone(),
            format: crate::display::TranscriptFormat::UnknownFallback,
            tone: DisplayTone::Normal,
            copy_source: source.clone(),
            streaming: false,
        }),
        None,
    );
    state.reconcile_latest_preview();
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    let buffer = terminal.backend().buffer();
    // Border, title, and pane background are removed. A newly selected
    // non-reasoning Ready target fades in as one complete block.
    assert_ne!(buffer[(72, 0)].symbol(), "┌");
    let preview_text = (0..30)
        .flat_map(|y| (72..120).map(move |x| buffer[(x, y)].symbol()))
        .collect::<String>();
    assert!(preview_text.contains("# Preview source"));
    assert!(preview_text.contains("complete body"));
    let unit = state.transcript.nodes()[0]
        .unit()
        .expect("materialized unit");
    assert!(state.render.units.contains_key(&unit));
    assert!(matches!(
        &state.transcript.nodes()[0].item,
        DisplayItem::Block(block) if block.copy_source == source
    ));
    assert!(provenance_layout_rows(&state)
        .iter()
        .any(|row| row.unit == unit));
}

#[test]
fn history_stays_in_message_pane_and_keeps_preview_live() {
    use crate::history_page::{HistoryLoadState, HistoryPage};

    let mut state = TuiApp::default();
    state.config.message_pane_percent = crate::PaneWidthPercent::from_basis_points(5_000).unwrap();
    state.preview.state = PreviewState::Ready(PreviewContent::PlainText("preview sentinel".into()));
    let mut input = InputState::new(&state.config);
    input.restore_text("preserved draft".into());
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let before = terminal.backend().buffer().clone();
    let cache_rebuilds = state.render.transcript_cache.work.rebuilds;
    let separator = separator_column(&state, 120) as u16;
    state.history_page = Some(HistoryPage::loading(1, "session".into(), "root".into()));

    for load_state in [
        HistoryLoadState::Loading,
        HistoryLoadState::Empty,
        HistoryLoadState::Error("long error ".repeat(30)),
        HistoryLoadState::Ready,
    ] {
        state.history_page.as_mut().unwrap().state = load_state;
        terminal
            .draw(|frame| {
                assert_eq!(
                    render_with_cursor(frame, &mut state, &input, &mut scroll, &theme, overlays()),
                    None
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(find_text(buffer, "preserved draft").is_none());
        for y in 0..30 {
            for x in separator..120 {
                assert_eq!(buffer[(x, y)], before[(x, y)], "Preview changed at {x},{y}");
            }
        }
        assert_eq!(state.history_page.as_ref().unwrap().body_height, 27);
        assert_eq!(state.render.transcript_cache.work.rebuilds, cache_rebuilds);
    }

    state.preview.state = PreviewState::Ready(PreviewContent::PlainText("updated preview".into()));
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let (x, _) = find_text(terminal.backend().buffer(), "updated preview").unwrap();
    assert!(x > separator);

    state.history_page = None;
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(find_text(terminal.backend().buffer(), "preserved draft").is_some());
}

#[test]
fn history_resize_preserves_narrow_preview_mode_for_exit() {
    use crate::history_page::HistoryPage;

    let mut state = TuiApp::default();
    state.preview.fullscreen = true;
    let saved_percent = state.config.message_pane_percent;
    state.preview.state = PreviewState::Ready(PreviewContent::PlainText("preview sentinel".into()));
    state.history_page = Some(HistoryPage::loading(1, "session".into(), "root".into()));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();

    for width in [120, 40, 120, 40] {
        terminal.backend_mut().resize(width, 30);
        terminal.autoresize().unwrap();
        terminal
            .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(find_text(buffer, "Loading execution history").is_some());
        assert_eq!(
            find_text(buffer, "preview sentinel").is_some(),
            width == 120
        );
        assert!(state.preview.fullscreen);
        assert_eq!(state.config.message_pane_percent, saved_percent);
    }

    state.history_page = None;
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(find_text(buffer, "Loading execution history").is_none());
    assert_eq!(find_text(buffer, "preview sentinel").unwrap().0, 1);
}

#[test]
fn split_preview_keeps_a_blank_column_after_the_separator() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.preview.state = PreviewState::Ready(PreviewContent::PlainText("preview".into()));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let (text_x, text_y) = find_text(buffer, "preview").expect("Preview text renders");
    let separator_x = separator_column(&state, 120) as u16;
    assert_eq!(
        text_x,
        separator_x + 2,
        "split Preview leaves one blank cell after the separator"
    );
    assert_eq!(
        buffer[(separator_x, text_y)].symbol(),
        "│",
        "separator remains at the live pane boundary"
    );
    assert_eq!(
        buffer[(separator_x + 1, text_y)].symbol(),
        " ",
        "separator gap is blank"
    );
    assert_eq!(
        buffer[(119, text_y)].symbol(),
        " ",
        "Preview keeps one right margin"
    );
}

#[test]
fn tool_preview_renders_header_primary_and_secondary_with_ferra_semantics() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.policy = crate::preview::PreviewPolicy::FollowReadingCursor;
    state.preview.state = PreviewState::Ready(PreviewContent::Tool(ToolPreview {
        name: "bash".into(),
        primary: ToolPreviewPrimary::Command {
            command: "grep -R table".into(),
            metrics: ToolMetrics {
                output_lines: 2,
                truncated: false,
                duration_ms: Some(1200),
            },
        },
        secondary: Some(ToolPreviewSecondary::Terminal {
            output: "plain \u{1b}[31mred\u{1b}[0m".into(),
            truncated: false,
        }),
    }));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let area = *buffer.area();
    let find = |needle: &str| -> Option<(u16, u16)> {
        for y in 0..area.height {
            for x in 0..area.width.saturating_sub(needle.chars().count() as u16) {
                let matched = needle
                    .chars()
                    .enumerate()
                    .all(|(offset, ch)| buffer[(x + offset as u16, y)].symbol() == ch.to_string());
                if matched {
                    return Some((x, y));
                }
            }
        }
        None
    };
    let (name_x, name_y) = find("bash").expect("tool name header renders");
    assert_eq!(name_y, 3, "short tool Preview remains vertically centered");
    let (dollar_x, dollar_y) = find("$").expect("command prompt renders");
    let (metrics_x, metrics_y) = find("lines 2, 1.2s").expect("metrics render");
    assert!(find("duration").is_none(), "duration label must be omitted");
    assert!(find("grep -R table").is_some());
    assert!(
        find("plain").is_some() && find("red").is_some(),
        "terminal output renders"
    );
    // No blank row between the name header and the primary `$` row.
    assert_eq!(dollar_y, name_y + 1);
    // One blank row between the metrics row and the secondary terminal output.
    assert_eq!(dollar_y + 1, metrics_y);
    // Ferra semantics: name = activity.label (umber), `$` = prompt (coral),
    // metrics = activity.detail (bark).
    assert_eq!(buffer[(name_x, name_y)].fg, theme.activity.label.fg);
    assert_eq!(buffer[(dollar_x, dollar_y)].fg, theme.input.prompt.fg);
    assert_eq!(buffer[(metrics_x, metrics_y)].fg, theme.activity.detail.fg);
}

#[test]
fn selected_tool_preview_reveals_as_one_block_without_touching_transcript_cache() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.policy = crate::preview::PreviewPolicy::FollowReadingCursor;
    state.render.transcript_cache.valid = true;
    state.preview.select(Some(crate::preview::PreviewTarget {
        id: "tool:bash".into(),
        reference: crate::preview::PreviewRef::Inline {
            key: crate::preview::PreviewKey("tool:bash".into()),
            revision: crate::preview::PreviewRevision(1),
            content: PreviewContent::Tool(ToolPreview {
                name: "bash".into(),
                primary: ToolPreviewPrimary::Command {
                    command: "echo ok".into(),
                    metrics: ToolMetrics::default(),
                },
                secondary: None,
            }),
        },
    }));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("bash"));
    assert!(text.contains("echo ok"));
    let first_cell = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .find(|cell| cell.symbol() == "b")
        .expect("first tool-name row");
    assert_eq!(
        first_cell.fg,
        crate::reveal::blend_rgb(
            state.config.background_color.color(),
            theme.activity.label.fg,
            crate::reveal::TEXT_FADE_WEIGHTS[0],
        )
    );

    for _ in 0..2 {
        let fade_due = state
            .preview
            .reveal_deadline()
            .expect("block fade is active");
        assert!(state
            .preview
            .tick_reveal(fade_due, state.config.preview_lines_per_second.get()));
    }
    assert_eq!(state.preview.reveal_deadline(), None);
    assert!(state.render.transcript_cache.valid);
}

#[test]
fn fresh_live_reasoning_preview_remains_row_paced() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.select(Some(crate::preview::PreviewTarget {
        id: "reasoning:live".into(),
        reference: crate::preview::PreviewRef::Inline {
            key: crate::preview::PreviewKey("reasoning:live".into()),
            revision: crate::preview::PreviewRevision(1),
            content: PreviewContent::Reasoning("first row\nsecond row".into()),
        },
    }));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(find_text(terminal.backend().buffer(), "first row").is_some());
    assert!(find_text(terminal.backend().buffer(), "second row").is_none());

    for _ in 0..4 {
        let due = state
            .preview
            .reveal_deadline()
            .expect("reasoning has pending reveal");
        state
            .preview
            .tick_reveal(due, state.config.preview_lines_per_second.get());
        terminal
            .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
            .unwrap();
        if find_text(terminal.backend().buffer(), "second row").is_some() {
            break;
        }
    }
    assert!(find_text(terminal.backend().buffer(), "second row").is_some());
}

#[test]
fn streaming_preview_code_keeps_visible_rows_when_fence_header_changes() {
    let mut state = TuiApp::default();
    force_preview_only(&mut state);
    let target = |revision, source: &str| crate::preview::PreviewTarget {
        id: "reasoning:code".into(),
        reference: crate::preview::PreviewRef::Inline {
            key: crate::preview::PreviewKey("reasoning:code".into()),
            revision: crate::preview::PreviewRevision(revision),
            content: PreviewContent::Reasoning(source.into()),
        },
    };
    let mut source = "```rust\nlet first = 1;".to_owned();
    state.preview.select(Some(target(1, &source)));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    for revision in 2..=5 {
        for _ in 0..32 {
            let Some(due) = state.preview.reveal_deadline() else {
                break;
            };
            state
                .preview
                .tick_reveal(due, state.config.preview_lines_per_second.get());
        }
        source.push_str("\nlet next = 2;");
        state.preview.select(Some(target(revision, &source)));
        terminal
            .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
            .unwrap();
        assert!(
            find_text(terminal.backend().buffer(), "let first = 1;").is_some(),
            "revision {revision} replayed visible code"
        );
    }
}

#[test]
fn reading_reasoning_preview_fades_as_one_complete_page() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.select_with_intent(
        Some(crate::preview::PreviewTarget {
            id: "reasoning:history".into(),
            reference: crate::preview::PreviewRef::Inline {
                key: crate::preview::PreviewKey("reasoning:history".into()),
                revision: crate::preview::PreviewRevision(1),
                content: PreviewContent::Reasoning("first row\nsecond row".into()),
            },
        }),
        crate::preview::PreviewRevealIntent::Page,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(find_text(terminal.backend().buffer(), "first row").is_some());
    assert!(find_text(terminal.backend().buffer(), "second row").is_some());
}

#[test]
fn long_tool_output_pins_information_and_shows_the_latest_tail() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.state = PreviewState::Ready(PreviewContent::Tool(ToolPreview {
        name: "bash".into(),
        primary: ToolPreviewPrimary::Command {
            command: "echo output".into(),
            metrics: ToolMetrics {
                output_lines: 20,
                truncated: false,
                duration_ms: Some(1000),
            },
        },
        secondary: Some(ToolPreviewSecondary::Terminal {
            output: (1..=20)
                .map(|line| format!("output-{line:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
            truncated: false,
        }),
    }));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(find_text(buffer, "bash").map(|(_, y)| y), Some(0));
    assert_eq!(find_text(buffer, "$ echo output").map(|(_, y)| y), Some(1));
    assert!(find_text(buffer, "output-20").is_some());
    assert!(find_text(buffer, "output-01").is_none());
}

#[test]
fn terminal_output_is_clipped_without_wrap_or_ellipsis() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.select(Some(crate::preview::PreviewTarget {
        id: "tool:nowrap".into(),
        reference: crate::preview::PreviewRef::Inline {
            key: crate::preview::PreviewKey("tool:nowrap".into()),
            revision: crate::preview::PreviewRevision(1),
            content: PreviewContent::Tool(ToolPreview {
                name: "bash".into(),
                primary: ToolPreviewPrimary::Command {
                    command: "x".into(),
                    metrics: ToolMetrics {
                        output_lines: 2,
                        truncated: true,
                        duration_ms: None,
                    },
                },
                secondary: Some(ToolPreviewSecondary::Terminal {
                    output: "0123456789ABCDEFGHIJ-TAIL\nNEXT".into(),
                    truncated: true,
                }),
            }),
        },
    }));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (_, first_output_y) = find_text(buffer, "0123456789").expect("first output row");
    let (_, next_output_y) = find_text(buffer, "NEXT").expect("second output row");
    assert_eq!(
        next_output_y,
        first_output_y + 1,
        "one terminal source row must occupy exactly one display row"
    );
    assert!(find_text(buffer, "TAIL").is_none());
    assert!(find_text(buffer, "…").is_none());
}

#[test]
fn wrapped_tool_information_takes_priority_over_output() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.state = PreviewState::Ready(PreviewContent::Tool(ToolPreview {
        name: "bash".into(),
        primary: ToolPreviewPrimary::Command {
            command: "a very long command that wraps across every available preview row".into(),
            metrics: ToolMetrics::default(),
        },
        secondary: Some(ToolPreviewSecondary::Terminal {
            output: "OUTPUT-MUST-WAIT".into(),
            truncated: false,
        }),
    }));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(find_text(buffer, "bash").map(|(_, y)| y), Some(0));
    assert!(find_text(buffer, "OUTPUT").is_none());
}

#[test]
fn narrow_fullscreen_preview_renders_loading_and_error_states() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.policy = crate::preview::PreviewPolicy::FollowReadingCursor;
    state.preview.state = PreviewState::Loading {
        request_id: crate::preview::PreviewRequestId(1),
        key: crate::preview::PreviewKey("file".into()),
        revision: crate::preview::PreviewRevision(1),
    };
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let content = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(content.contains("Loading preview"));

    state.preview.state = PreviewState::Error("bounded failure".into());
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 5)].fg, theme.log.error.fg);
}

#[test]
fn reading_block_navigation_preserves_draft_preview_and_highlight_geometry() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    for index in 0..3 {
        let text = format!("block {index} with wrapped semantic content");
        state.transcript.append(
            DisplayItem::Block(crate::display::TranscriptBlock {
                id: DisplayId::correlated("assistant", &index.to_string()),
                unit: Some(20 + index),
                content: text.clone(),
                format: crate::display::TranscriptFormat::Plain,
                tone: DisplayTone::Normal,
                copy_source: text,
                streaming: false,
            }),
            None,
        );
    }
    let mut input = InputState::new(&state.config);
    input.buf = "unfinished\ndraft".into();
    input.cursor = 5;
    input.multiline = true;
    let original = input.clone();
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 18)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(state.enter_reading(&input, &mut scroll, 12));
    assert!(
        !state.render.transcript_cache.valid,
        "Reading entry invalidates the folded normal-mode layout"
    );
    let initially_selected = state.reading.as_ref().unwrap().block_cursor.clone();
    assert_eq!(
        state.preview.policy,
        crate::preview::PreviewPolicy::FollowReadingCursor
    );
    assert!(state.reading_copy_text().unwrap().starts_with("block"));

    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(state.render.transcript_cache.valid);
    let current = state.reading.as_ref().expect("reading remains active");
    let geometry = state
        .reading_layout
        .block(&current.block_cursor)
        .expect("current geometry");
    assert!(!geometry.rows.is_empty());
    let buffer = terminal.backend().buffer();
    let rails = (0..18)
        .flat_map(|y| (0..70).map(move |x| (x, y)))
        .filter(|(x, y)| buffer[(*x, *y)].symbol() == "│")
        .collect::<Vec<_>>();
    assert!(
        !rails.is_empty(),
        "missing reading rail; x3={:?}",
        (0..18)
            .map(|y| buffer[(3, y)].symbol().to_owned())
            .collect::<Vec<_>>()
    );
    assert!((0..18).any(|y| (4..66).any(|x| buffer[(x, y)].bg == theme.bg)));

    let moved = state.move_reading_block(-1, &mut scroll, 12)
        || state.move_reading_block(1, &mut scroll, 12);
    assert!(moved);
    while state.move_reading_block(1, &mut scroll, 12) {}
    let boundary = state.reading.as_ref().unwrap().block_cursor.clone();
    assert!(!state.move_reading_block(1, &mut scroll, 12));
    assert_eq!(state.reading.as_ref().unwrap().block_cursor, boundary);
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "live"),
            unit: Some(99),
            content: "live append".into(),
            format: crate::display::TranscriptFormat::Plain,
            tone: DisplayTone::Normal,
            copy_source: "live append".into(),
            streaming: false,
        }),
        None,
    );
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert_eq!(state.reading.as_ref().unwrap().block_cursor, boundary);
    assert!(!initially_selected.0.is_empty());

    input.buf = "mutated".into();
    state.exit_reading(&mut input);
    assert!(
        !state.render.transcript_cache.valid,
        "Reading exit invalidates the expanded layout"
    );
    assert_eq!(input.buf, original.buf);
    assert_eq!(input.cursor, original.cursor);
    assert!(input.multiline);
    assert_eq!(
        state.preview.policy,
        crate::preview::PreviewPolicy::FollowLatestBlock
    );
}

#[test]
fn preview_content_kinds_materialize_visible_rows_only() {
    let mut state = TuiApp::default();
    force_preview_only(&mut state);
    state.preview.policy = crate::preview::PreviewPolicy::FollowReadingCursor;
    state.preview.scroll = 20;
    state.preview.state = PreviewState::Ready(PreviewContent::Lines {
        path: "src/main.rs".into(),
        start: 1,
        lines: (1..=100).map(|line| format!("line {line}")).collect(),
    });
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = state.theme();
    let mut terminal = Terminal::new(TestBackend::new(70, 10)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let content = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(content.contains("line 20") || content.contains("line 21"));
    assert!(!content.contains("line 100"));
}

#[test]
fn preview_follow_anchor_keeps_latest_content_visible_when_overflowing() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.policy = crate::preview::PreviewPolicy::FollowLatestBlock;
    // More rows than the pane can show; scroll == 0 means follow-the-latest,
    // so streaming reasoning must stay bottom-anchored instead of showing the
    // head and hiding the newest rows.
    let source = (1..=60)
        .map(|index| format!("line {index:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    state.preview.state = PreviewState::Ready(PreviewContent::PlainText(source));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 20)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let content = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        content.contains("line 60"),
        "streaming reasoning tail must stay visible"
    );
    assert!(!content.contains("line 01"), "head must be scrolled out");
}

#[test]
fn preview_wraps_long_lines_to_the_pane_width() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    // A single 200-char line far exceeds the 68-column padded content width;
    // it must wrap into multiple rows instead of truncating at the pane edge.
    state.preview.state = PreviewState::Ready(PreviewContent::PlainText("a".repeat(200)));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 20)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let a_cells = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .filter(|cell| cell.symbol() == "a")
        .count();
    assert_eq!(a_cells, 200, "long preview text must wrap, not truncate");
}

#[test]
fn reasoning_preview_renders_with_weak_markdown_semantics() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.state =
        PreviewState::Ready(PreviewContent::Reasoning("**bold** *italic* `code`".into()));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 20)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let content = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!content.contains('*'), "bold markers must render, not leak");
    assert!(!content.contains('`'), "code markers must render, not leak");
    // Single centered row: top padding (20-1)/2 = 9, text starts at x=1.
    let bold = &terminal.backend().buffer()[(1, 9)];
    assert_eq!(
        bold.fg, theme.markdown_weak.strong.fg,
        "bold span uses weak Markdown"
    );
    assert!(
        bold.modifier.contains(ratatui::style::Modifier::BOLD),
        "bold modifier preserved"
    );
    let code = &terminal.backend().buffer()[(13, 9)];
    assert_eq!(
        code.fg, theme.markdown_weak.inline_code.fg,
        "inline code uses weak Markdown"
    );
    assert_eq!(
        code.bg,
        theme.markdown_weak.inline_code.bg.unwrap_or(Color::Reset),
        "weak inline code chip background preserved"
    );
}

#[test]
fn preview_styled_layout_caches_syntax_until_width_or_theme_changes() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.state = PreviewState::Ready(PreviewContent::Markdown(
        "```rust\nfn main() {}\n```".into(),
    ));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let mut theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 20)).unwrap();

    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert_eq!(state.preview.take_work_stats().layout_rebuilds, 1);
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert_eq!(
        state.preview.take_work_stats().layout_rebuilds,
        0,
        "unchanged redraw reuses styled rows"
    );

    let mut terminal = Terminal::new(TestBackend::new(72, 20)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert_eq!(state.preview.take_work_stats().layout_rebuilds, 1);

    theme = Theme::from_name("dracula");
    state.config.resolved_theme = theme;
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert_eq!(state.preview.take_work_stats().layout_rebuilds, 1);
}

#[test]
fn diff_preview_composes_normal_syntax_with_added_and_removed_backgrounds() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.state = PreviewState::Ready(PreviewContent::Diff {
        path: Some("src/main.rs".into()),
        source: "@@ -1 +1 @@\n-fn old() {}\n+fn new() {}".into(),
    });
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 20)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (old_x, old_y) = find_text(buffer, "fn old").expect("removed Rust row");
    let (new_x, new_y) = find_text(buffer, "fn new").expect("added Rust row");
    assert_eq!(buffer[(old_x, old_y)].fg, theme.code.keyword.fg);
    assert_eq!(buffer[(old_x, old_y)].bg, theme.diff.removed.bg.unwrap());
    assert_eq!(buffer[(new_x, new_y)].fg, theme.code.keyword.fg);
    assert_eq!(buffer[(new_x, new_y)].bg, theme.diff.added.bg.unwrap());
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(content.contains("▌    1 │ fn old"));
    assert!(content.contains("▌    1 │ fn new"));
}

#[test]
fn wrapped_markdown_list_rows_align_under_the_item_text() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let source = "- alpha bravo charlie delta echo foxtrot golf hotel india";
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "wrapped-list"),
            unit: None,
            content: source.into(),
            format: crate::display::TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: source.into(),
            streaming: false,
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    // The cached rows are what both the painter and copy provenance consume:
    // the item wraps at render time and no row overflows the page width.
    let width = state.render.transcript_cache.width;
    let rows = state
        .render
        .transcript_cache
        .lines
        .iter()
        .map(Line::to_string)
        .filter(|row| !row.trim().is_empty())
        .collect::<Vec<_>>();
    assert!(rows.len() > 1, "the long item must wrap: {rows:?}");
    assert!(rows[0].starts_with("◦ alpha"), "marker row: {:?}", rows[0]);
    for row in &rows[1..] {
        assert!(
            row.starts_with("  ") && !row.starts_with("   "),
            "continuation row hangs in the text column: {row:?}"
        );
    }
    for row in &rows {
        assert!(
            unicode_width::UnicodeWidthStr::width(row.as_str()) <= width,
            "row exceeds the {width}-column page: {row:?}"
        );
    }

    // Painted geometry: continuation rows start exactly where the item text
    // starts, not at the page edge under the marker. Columns are counted in
    // cells (one char per cell in `row_text`), never raw byte offsets — the
    // `◦` marker is three bytes wide.
    let buffer = terminal.backend().buffer();
    let row_text = |y: u16| {
        (0..40u16)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    let column_of = |row: &str, needle: &str| {
        row.find(needle)
            .map(|byte| row[..byte].chars().count())
            .expect("painted needle")
    };
    let marker_row = (0..12u16)
        .find(|y| row_text(*y).contains('◦'))
        .expect("marker row painted");
    let text_x = column_of(&row_text(marker_row), "alpha");
    let continuation = row_text(marker_row + 1);
    assert_eq!(
        continuation.chars().position(|c| c != ' '),
        Some(text_x),
        "continuation row starts in the text column: {continuation:?}"
    );
}

#[test]
fn cjk_markdown_block_wraps_at_ideograph_boundaries() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    // No spaces anywhere: UAX #14 supplies the break opportunities between
    // ideographs, keeps the fullwidth comma glued to the preceding character,
    // and leaves the trailing Latin word intact.
    let source = "你好，世界你好，世界你好，世界abc";
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "cjk-wrap"),
            unit: None,
            content: source.into(),
            format: crate::display::TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: source.into(),
            streaming: false,
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    // Plain paragraphs stay unwrapped in the cache by design; assert on the
    // final painted rows, where the paint-time wrapper applied the breaks.
    // Rebuild each row from glyph cells only: double-width characters leave
    // empty continuation cells and padding cells hold spaces.
    let buffer = terminal.backend().buffer();
    let text_rows = (0..12u16)
        .map(|y| {
            (0..20u16)
                .map(|x| buffer[(x, y)].symbol())
                .filter(|symbol| !symbol.is_empty() && *symbol != " ")
                .collect::<String>()
        })
        .filter(|row| !row.is_empty() && (row.contains('你') || row.contains("abc")))
        .collect::<Vec<_>>();
    assert_eq!(
        text_rows.len(),
        3,
        "expected three wrapped rows: {text_rows:?}"
    );
    assert_eq!(
        text_rows.concat(),
        source,
        "wrapping must not drop or reorder text"
    );
    for row in &text_rows {
        assert!(
            !row.starts_with('，'),
            "fullwidth comma must never start a painted row: {text_rows:?}"
        );
    }
    // The trailing Latin word stays whole on its final row.
    assert!(
        text_rows.last().expect("painted rows").ends_with("abc"),
        "Latin word must stay intact: {text_rows:?}"
    );
}

#[test]
fn wrapped_markdown_quote_rows_keep_the_painted_gutter() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let source = "> alpha bravo charlie delta echo foxtrot golf hotel india";
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "wrapped-quote"),
            unit: None,
            content: source.into(),
            format: crate::display::TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: source.into(),
            streaming: false,
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();

    let width = state.render.transcript_cache.width;
    let rows = state
        .render
        .transcript_cache
        .lines
        .iter()
        .map(Line::to_string)
        .filter(|row| !row.trim().is_empty())
        .collect::<Vec<_>>();
    assert!(rows.len() > 1, "the long quote line must wrap: {rows:?}");
    for row in &rows {
        assert!(row.starts_with("│ "), "every row keeps its bar: {row:?}");
        assert!(
            unicode_width::UnicodeWidthStr::width(row.as_str()) <= width,
            "row exceeds the {width}-column page: {row:?}"
        );
    }

    // Painted geometry: the bar column is identical on the wrapped row and
    // keeps the quote marker tone, so the gutter reads as one vertical line.
    let buffer = terminal.backend().buffer();
    let bar_column = |y: u16| (0..40u16).find(|x| buffer[(*x, y)].symbol() == "│");
    let first = (0..12u16)
        .find(|y| bar_column(*y).is_some())
        .expect("quote row painted");
    let bar_x = bar_column(first).expect("bar painted");
    assert_eq!(
        bar_column(first + 1),
        Some(bar_x),
        "the wrapped row repeats the bar in the same column"
    );
    assert_eq!(
        buffer[(bar_x, first + 1)].fg,
        theme.markdown.quote_marker.fg,
        "wrapped bar keeps the quote marker tone"
    );
    assert_eq!(
        buffer[(bar_x + 2, first + 1)].symbol(),
        "f",
        "wrapped text (`foxtrot …`) starts right after the bar"
    );
}

/// Regression (6cdb025b): the pending-prompt queue and the approval card are
/// passed through RenderOverlays (the main loop holds the InteractionModel out
/// of AppState while drawing). The render must paint them instead of reading
/// the transient `state.interaction`.
#[test]
fn overlays_paint_queue_and_approval_accessories() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let card = crate::interaction::ApprovalCard {
        id: "a1".into(),
        tool_name: "bash".into(),
        reason: "run the test".into(),
    };
    let queue = vec![crate::interaction::PendingPrompt::new(
        "排队提示".into(),
        crate::interaction::PromptDelivery::Asap,
    )];
    let backend = TestBackend::new(80, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render_with_cursor(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                RenderOverlays {
                    help_visible: false,
                    toast: None,
                    input_page: None,
                    settings: None,
                    login: None,
                    approval: Some(&card),
                    queue: &queue,
                    pane_resize: Default::default(),
                },
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..40u16)
        .map(|y| {
            (0..80u16)
                .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Cell extraction spaces wide CJK glyphs; strip whitespace for matching.
    let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("Approval") && flat.contains("bash"),
        "approval card painted from overlays; screen:\n{text}"
    );
    assert!(
        flat.contains("排队提示"),
        "queued prompt strip painted from overlays; screen:\n{text}"
    );
}

#[test]
fn chinese_approval_and_queue_accessories_localize_chrome() {
    let mut state = TuiApp::default();
    state.config.language = crate::Language::SimplifiedChinese;
    state.config.resolved_theme = Theme::ferra();
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let card = crate::interaction::ApprovalCard {
        id: "a1".into(),
        tool_name: "bash".into(),
        reason: "run the test".into(),
    };
    let queue = (1..=10)
        .map(|index| {
            crate::interaction::PendingPrompt::new(
                format!("queued {index}").into(),
                crate::interaction::PromptDelivery::AfterTurn,
            )
        })
        .collect::<Vec<_>>();
    let mut terminal = Terminal::new(TestBackend::new(80, 15)).unwrap();
    terminal
        .draw(|frame| {
            render_with_cursor(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                RenderOverlays {
                    approval: Some(&card),
                    queue: &queue,
                    ..overlays()
                },
            );
        })
        .unwrap();

    let flat = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert!(
        flat.contains("审批"),
        "approval title is not localized: {flat}"
    );
    assert!(flat.contains("queued1"), "queued prompt changed: {flat}");
    assert!(
        flat.contains("还有"),
        "queue overflow is not localized: {flat}"
    );
}

#[test]
fn mouse_selection_highlights_transcript_wide_cells_over_reading_style() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let source = "A界B";
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "mouse-reading"),
            unit: Some(71),
            content: source.into(),
            format: crate::display::TranscriptFormat::Plain,
            tone: DisplayTone::Normal,
            copy_source: source.into(),
            streaming: false,
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 14)).unwrap();

    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(state.enter_reading(&input, &mut scroll, 9));
    let mut rendered = None;
    terminal
        .draw(|frame| {
            rendered = Some(render_with_cursor_and_selection(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                overlays(),
                &MouseSelection::default(),
                &Presentation::default(),
            ));
        })
        .unwrap();
    let mut committed = Presentation::default();
    committed.commit(
        rendered.expect("render returns presentation").presentation,
        &mut MouseSelection::default(),
    );

    let ((a_x, y), wide_x, b_x, baseline) = {
        let buffer = terminal.backend().buffer();
        let (a_x, y) = find_text(buffer, "A").expect("transcript text renders");
        let wide_x = a_x + 1;
        let b_x = a_x + 3;
        assert_eq!(buffer[(wide_x, y)].symbol(), "界");
        assert_eq!(buffer[(b_x, y)].symbol(), "B");
        (
            (a_x, y),
            wide_x,
            b_x,
            (buffer[(wide_x, y)].fg, buffer[(wide_x, y)].bg),
        )
    };
    let canonical = state.reading_copy_text().expect("Reading copy payload");
    let mut selection = MouseSelection::default();
    selection.handle(
        PointerEvent::PrimaryPress {
            column: a_x,
            row: y,
        },
        committed.selection_frame(),
    );
    let copied = selection.handle(
        PointerEvent::PrimaryRelease {
            column: b_x,
            row: y,
        },
        committed.selection_frame(),
    );
    assert_eq!(
        copied.copy.as_deref(),
        Some(source),
        "coords=({a_x},{y})..({b_x},{y})"
    );

    terminal
        .draw(|frame| {
            render_with_cursor_and_selection(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                overlays(),
                &selection,
                &committed,
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    for x in [a_x, b_x] {
        assert!(
            buffer[(x, y)].modifier.contains(Modifier::REVERSED),
            "selected boundary cell {x},{y} is reversed"
        );
    }
    assert_eq!(buffer[(wide_x, y)].symbol(), "界");
    assert_eq!(
        buffer[(wide_x + 1, y)].symbol(),
        " ",
        "Ratatui keeps the second terminal cell covered by the selected wide glyph"
    );
    assert_eq!(buffer[(wide_x, y)].fg, baseline.0);
    assert_eq!(buffer[(wide_x, y)].bg, baseline.1);
    assert_eq!(
        state.reading_copy_text().as_deref(),
        Some(canonical.as_str()),
        "mouse presentation does not alter Reading complete-source copy"
    );
}

#[test]
fn mouse_selection_crosses_preview_and_transcript_in_screen_order() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "mouse-main"),
            unit: Some(72),
            content: "main text".into(),
            format: crate::display::TranscriptFormat::Plain,
            tone: DisplayTone::Normal,
            copy_source: "main text".into(),
            streaming: false,
        }),
        None,
    );
    state.preview.state = PreviewState::Ready(PreviewContent::Reasoning("preview text".into()));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    let mut rendered = None;
    terminal
        .draw(|frame| {
            rendered = Some(render_with_cursor_and_selection(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                overlays(),
                &MouseSelection::default(),
                &Presentation::default(),
            ));
        })
        .unwrap();
    let mut committed = Presentation::default();
    committed.commit(
        rendered
            .expect("render returns split presentation")
            .presentation,
        &mut MouseSelection::default(),
    );
    state.render.transcript_cache.take_work_stats();
    state.preview.take_work_stats();
    let ((main_x, main_y), (preview_x, preview_y)) = {
        let buffer = terminal.backend().buffer();
        (
            find_text(buffer, "main text").expect("transcript text renders"),
            find_text(buffer, "preview text").expect("Preview text renders"),
        )
    };

    let mut selection = MouseSelection::default();
    selection.handle(
        PointerEvent::PrimaryPress {
            column: preview_x,
            row: preview_y,
        },
        committed.selection_frame(),
    );
    let preview_copy = selection.handle(
        PointerEvent::PrimaryRelease {
            column: preview_x + 6,
            row: preview_y,
        },
        committed.selection_frame(),
    );
    assert_eq!(preview_copy.copy.as_deref(), Some("preview"));
    terminal
        .draw(|frame| {
            render_with_cursor_and_selection(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                overlays(),
                &selection,
                &committed,
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer[(preview_x, preview_y)]
        .modifier
        .contains(Modifier::REVERSED));
    assert!(!buffer[(main_x, main_y)]
        .modifier
        .contains(Modifier::REVERSED));

    selection.handle(
        PointerEvent::PrimaryPress {
            column: main_x,
            row: main_y,
        },
        committed.selection_frame(),
    );
    let copied = selection.handle(
        PointerEvent::PrimaryRelease {
            column: preview_x,
            row: preview_y,
        },
        committed.selection_frame(),
    );
    let copied = copied.copy.expect("cross-pane range copies");
    assert!(copied.starts_with("main text"));
    assert!(copied.ends_with('p'));
    assert!(copied.contains('\n'));
    terminal
        .draw(|frame| {
            render_with_cursor_and_selection(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                overlays(),
                &selection,
                &committed,
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer[(main_x, main_y)]
        .modifier
        .contains(Modifier::REVERSED));
    assert!(buffer[(preview_x, preview_y)]
        .modifier
        .contains(Modifier::REVERSED));
    let transcript_work = state.render.transcript_cache.take_work_stats();
    let preview_work = state.preview.take_work_stats();
    assert_eq!(transcript_work.rebuilds, 0);
    assert_eq!(transcript_work.patches, 0);
    assert_eq!(preview_work.rebuilds, 0);
    assert_eq!(preview_work.patches, 0);
}

#[test]
fn pane_separator_uses_explicit_background_in_normal_mode() {
    let mut state = TuiApp::default();
    let mut theme = Theme::ferra();
    theme.separator.bar.bg = Some(Color::Rgb(1, 2, 3));
    state.config.resolved_theme = theme;
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal
        .draw(|frame| {
            render(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let separator_x = separator_column(&state, 120) as u16;
    assert_eq!(buffer[(separator_x, 10)].symbol(), "│");
    assert_eq!(buffer[(separator_x, 10)].fg, theme.separator.bar.fg);
    assert_eq!(buffer[(separator_x, 10)].bg, Color::Rgb(1, 2, 3));
    assert_eq!(buffer[(separator_x, 0)].symbol(), " ");
}

#[test]
fn pane_separator_without_background_remains_transparent() {
    let mut state = TuiApp::default();
    let mut theme = Theme::ferra();
    theme.bg = Color::Rgb(4, 5, 6);
    theme.separator.bar.bg = None;
    state.config.resolved_theme = theme;
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal
        .draw(|frame| {
            render(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let separator_x = separator_column(&state, 120) as u16;
    assert_eq!(buffer[(separator_x, 10)].symbol(), "│");
    assert_eq!(buffer[(separator_x, 10)].bg, Color::Reset);
}

#[test]
fn pane_separator_drag_guide_without_background_remains_transparent() {
    let mut state = TuiApp::default();
    let mut theme = Theme::ferra();
    theme.bg = Color::Rgb(4, 5, 6);
    theme.separator.bar.bg = None;
    theme.separator.line.bg = None;
    state.config.resolved_theme = theme;
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let mut pane_resize = crate::interaction::PaneResizeState::default();
    let separator_x = separator_column(&state, 120) as u16;
    assert!(pane_resize.begin(separator_x, state.config.message_pane_percent, false));
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal
        .draw(|frame| {
            render(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                RenderOverlays {
                    pane_resize,
                    ..overlays()
                },
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(separator_x, 0)].symbol(), "│");
    assert_eq!(buffer[(separator_x, 0)].bg, Color::Reset);
    assert_eq!(buffer[(separator_x, 10)].symbol(), "┃");
    assert_eq!(buffer[(separator_x, 10)].bg, Color::Reset);
}
