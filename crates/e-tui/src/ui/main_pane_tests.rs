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
    let text = row(text_row);
    let start_x = text.len() - text.trim_start().len();
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
    // 80 cols → split main 48 / preview 32 → page width 46 → inner 44 with
    // the default 1-column gutter. 200 chars wrap to 5 rows, so the box must
    // grow to the 5-row cap and the wrap window must follow the cursor: the
    // last five wrapped rows are visible and the IME anchor stays inside the
    // text area (previously the box stayed 3 rows tall, only the first chunk
    // was visible, and the anchor landed on the gap row below the bar).
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
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
    // Box = 5 text rows + 2 padding = 7 rows at the bottom: y 14..20
    // (bottom stack: 7 + 1 gap + 1 status + 1 title = 10).
    assert_eq!(&row(15)[2..46], "x".repeat(44), "visible row 1");
    assert_eq!(&row(16)[2..46], "x".repeat(44), "visible row 2");
    assert_eq!(&row(17)[2..46], "x".repeat(44), "visible row 3");
    assert_eq!(&row(18)[2..46], "x".repeat(44), "visible row 4");
    assert_eq!(
        &row(19)[2..26],
        "x".repeat(24),
        "cursor row shows the tail chunk"
    );
    assert_eq!(row(20).trim(), "", "bottom padding row of the box");
    assert_eq!(
        anchor,
        Some(Position::new(26, 19)),
        "IME anchor sits on the cursor row inside the text area"
    );
    // The reverse-video-style cursor block is drawn at the end of the tail
    // chunk (ferra cursor = fg night / bg mist).
    assert_eq!(
        buffer[(26, 19)].bg,
        theme
            .input
            .cursor
            .bg
            .expect("ferra cursor has a background"),
        "drawn cursor on the wrapped cursor row"
    );
}

#[test]
fn input_bar_full_final_row_keeps_cursor_out_of_bottom_padding() {
    // 180 chars fill four rows plus a four-character tail at inner 44. The
    // synthetic cursor space
    // after the final character must stay on the last text row (in the box's
    // right gutter), not wrap into the bottom padding row.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
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
    for y in 15..19 {
        assert_eq!(&row(y)[2..46], "x".repeat(44), "full text row {y}");
    }
    assert_eq!(
        &row(19)[2..6],
        "xxxx",
        "tail row stays inside the text area"
    );
    assert_eq!(row(20).trim(), "", "bottom padding row stays blank");
    assert_eq!(anchor, Some(Position::new(6, 19)));
    assert_eq!(
        buffer[(6, 19)].bg,
        theme
            .input
            .cursor
            .bg
            .expect("ferra cursor has a background"),
        "cursor block is patched into the right gutter on the last text row"
    );
}

#[test]
fn input_bar_multiline_wrapped_rows_keep_cursor_row_in_box() {
    // "a\n" + 200 y's: 7 display rows total, box capped at 5 text rows; the
    // window follows the cursor so the last five wrapped rows are shown.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
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
    assert_eq!(&row(15)[2..46], "y".repeat(44), "visible row 1");
    assert_eq!(&row(16)[2..46], "y".repeat(44), "visible row 2");
    assert_eq!(&row(17)[2..46], "y".repeat(44), "visible row 3");
    assert_eq!(&row(18)[2..46], "y".repeat(44), "visible row 4");
    assert_eq!(
        &row(19)[2..26],
        "y".repeat(24),
        "cursor row shows the tail chunk"
    );
    assert_eq!(
        anchor,
        Some(Position::new(26, 19)),
        "IME anchor stays on the cursor row inside the text area"
    );
}

#[test]
fn input_bar_fits_all_wrapped_rows_when_they_fit_the_box() {
    // 100 chars -> 3 wrapped rows at inner 44 -> the content still fits inside
    // the 5-row cap, so the whole content is visible from the top; the cursor
    // row is the last one.
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
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
    assert_eq!(&row(17)[2..46], "z".repeat(44), "row 1 from the top");
    assert_eq!(&row(18)[2..46], "z".repeat(44), "row 2 from the top");
    assert_eq!(&row(19)[2..14], "z".repeat(12), "row 3 from the top");
    assert_eq!(anchor, Some(Position::new(14, 19)));
}

#[test]
fn input_box_rows_follow_wrapped_content() {
    let config = crate::config::Config::default();
    let mut input = InputState::new(&config);
    // 40-column bar with a 2-column gutter on each side → inner 36.
    assert_eq!(input_rows(&input, 40, 2), 1, "empty input is one row");
    input.buf = "x".repeat(36);
    assert_eq!(input_rows(&input, 40, 2), 1, "exactly one row fits");
    input.buf = "x".repeat(37);
    assert_eq!(input_rows(&input, 40, 2), 2, "one overflow column wraps");
    input.buf = "x".repeat(200);
    assert_eq!(input_rows(&input, 40, 2), INPUT_MAX_ROWS, "cap at 5 rows");
    // Trailing newline still counts as an extra row (Shift+Enter growth).
    input.buf = "a\n".into();
    assert_eq!(input_rows(&input, 40, 2), 2);
    // A single long line wraps inside a narrow bar and grows the box.
    input.buf = "a very long single line that wraps".into();
    let rows = input_rows(&input, 16, 0);
    assert!(rows >= 2 && rows <= INPUT_MAX_ROWS);
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
        &row(18)[2..46],
        "aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa aaaa",
        "first wrapped row keeps the nine fitting words"
    );
    assert_eq!(
        &row(19)[2..6],
        "aaaa",
        "second wrapped row starts with the next whole word"
    );
    assert_eq!(
        anchor,
        Some(Position::new(36, 18)),
        "IME anchor sits after the first wrapped row at the consumed space"
    );
}

#[test]
fn extracted_main_pane_preserves_status_spacing_and_hidden_cursor() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
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

    assert_eq!(anchor, Some(Position::new(2, 35)));
    assert_eq!(terminal.get_cursor_position().unwrap(), Position::new(0, 0));
    let buffer = terminal.backend().buffer();
    let row = |y| {
        (0..80)
            .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };
    let main_row = |y| row(y).chars().take(48).collect::<String>();
    assert!(main_row(37).trim().is_empty());
    assert!(main_row(38).contains("• standard"));
    assert!(main_row(38).contains("^h Help"));
    assert!(main_row(39).contains("refactor bridge"));
    assert_eq!(buffer[(1, 38)].bg, Color::Reset);
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
    assert!(admitted.iter().any(|line| line.contains("a •")));

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
    assert!(second.iter().any(|line| line.contains("ab •")));
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
fn extracted_main_pane_keeps_card_background_and_copy_provenance() {
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

    let expected_bg = theme.card.user.bg.expect("user card background");
    let buffer = terminal.backend().buffer();
    let content_cells = (0..12)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .filter(|(x, y)| buffer[(*x, *y)].symbol() != " ")
        .filter(|(x, y)| buffer[(*x, *y)].symbol() != "│")
        .filter(|(x, y)| *y < 6 && *x >= 4)
        .collect::<Vec<_>>();
    assert!(!content_cells.is_empty());
    assert!(content_cells
        .iter()
        .all(|(x, y)| buffer[(*x, *y)].bg == expected_bg));
    assert_eq!(state.render.units.get(&7), Some(&source));
    let rows = provenance_layout_rows(&state);
    assert!(rows.iter().any(|row| row.unit == 7));
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
    // `提示词注入` label in the activity label tone (umber in ferra) and the
    // content in the activity detail tone (bark), with no background fill.
    assert_eq!(label(1), '提', "label glyph starts the first row");
    assert_eq!(label(3), '示', "label glyph on the first row");
    assert_eq!(label(5), '词', "label glyph on the first row");
    assert_eq!(label(7), '注', "label glyph on the first row");
    assert_eq!(label(9), '入', "label glyph on the first row");
    assert_eq!(
        buffer[(1, 0)].fg,
        theme.activity.label.fg,
        "label uses the activity label tone (umber)"
    );
    assert_eq!(
        buffer[(12, 0)].fg,
        theme.activity.detail.fg,
        "content uses the activity detail tone (bark)"
    );
    assert_eq!(buffer[(1, 0)].bg, Color::Reset, "no card background");
    assert_eq!(buffer[(12, 0)].bg, Color::Reset, "no card background");
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
    // Each CJK glyph occupies two cells, so compare per-cell glyphs and the
    // plain-ASCII content tail rather than a single raw substring.
    assert_eq!(buffer[(1, 0)].symbol(), "提");
    assert_eq!(buffer[(3, 0)].symbol(), "示");
    assert_eq!(buffer[(5, 0)].symbol(), "词");
    assert_eq!(buffer[(7, 0)].symbol(), "注");
    assert_eq!(buffer[(9, 0)].symbol(), "入");
    assert!(row_text(0).contains("short context"));
    assert!(row_text(1).trim().is_empty(), "one row then the gap");
    assert_eq!(buffer[(1, 0)].fg, theme.activity.label.fg);
    assert_eq!(buffer[(12, 0)].fg, theme.activity.detail.fg);
    assert_eq!(buffer[(1, 0)].bg, Color::Reset, "no card background");
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
            format: crate::display::TranscriptFormat::Plain,
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
    // Border, title, and pane background are removed. A newly selected Ready
    // target starts with its first wrapped display row.
    assert_ne!(buffer[(72, 0)].symbol(), "┌");
    let preview_text = (0..30)
        .flat_map(|y| (72..120).map(move |x| buffer[(x, y)].symbol()))
        .collect::<String>();
    assert!(preview_text.contains("# Preview source"));
    assert!(!preview_text.contains("complete body"));
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
    assert_eq!(text_x, 74, "split Preview leaves one blank cell after x=72");
    assert_eq!(
        buffer[(72, text_y)].symbol(),
        "│",
        "separator remains at x=72"
    );
    assert_eq!(buffer[(73, text_y)].symbol(), " ", "separator gap is blank");
    assert_eq!(
        buffer[(119, text_y)].symbol(),
        " ",
        "Preview keeps one right margin"
    );
}

#[test]
fn preview_skips_markdown_answers_and_shows_reasoning_text() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let preview_text = |terminal: &Terminal<TestBackend>| {
        (0..30)
            .flat_map(|y| (72..120).map(move |x| terminal.backend().buffer()[(x, y)].symbol()))
            .collect::<String>()
    };
    // Assistant markdown answers are rendered in the main pane and must not
    // drive the Preview pane.
    let source = "answer body".to_string();
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "markdown"),
            unit: Some(1),
            content: source.clone(),
            format: crate::display::TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: source.clone(),
            streaming: false,
        }),
        None,
    );
    state.reconcile_latest_preview();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(!preview_text(&terminal).contains("answer body"));
    assert!(matches!(state.preview.state, PreviewState::Empty));

    // The merged Thinking node with no streamed reasoning yet carries
    // nothing worth previewing.
    state.transcript.append(
        DisplayItem::Thinking(crate::display::ThinkingNode {
            row: crate::display::ActivityRow::root(
                DisplayId::correlated("thinking", "1"),
                "Thinking...",
            ),
            unit: None,
            content: String::new(),
            copy_source: String::new(),
            streaming: true,
            turn: None,
        }),
        None,
    );
    state.reconcile_latest_preview();
    assert!(
        matches!(state.preview.state, PreviewState::Empty),
        "empty Thinking node must not preview"
    );

    // Default `thinking_display` is compact, so reasoning is folded in the
    // main transcript; the Preview pane must still surface it live.
    let reasoning = "first reasoning delta".to_string();
    state.transcript.append(
        DisplayItem::Thinking(crate::display::ThinkingNode {
            row: crate::display::ActivityRow::root(
                DisplayId::correlated("thinking", "1"),
                "Thinking...",
            ),
            unit: Some(2),
            content: reasoning.clone(),
            copy_source: reasoning.clone(),
            streaming: true,
            turn: None,
        }),
        None,
    );
    state.reconcile_latest_preview();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert_eq!(
        state.preview.state,
        PreviewState::Ready(PreviewContent::Reasoning("first reasoning delta".into()))
    );
    assert!(preview_text(&terminal).contains("first reasoning delta"));
    // Reasoning keeps the Bark semantic target but the newest character is
    // initially blended from the configured background.
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(74, 14)].fg,
        crate::reveal::blend_rgb(
            state.config.background_color.color(),
            theme.surface.muted_text.fg,
            crate::reveal::TEXT_FADE_WEIGHTS[0],
        ),
        "reasoning preview starts with the shared fade"
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
    let (dollar_x, dollar_y) = find("$").expect("command prompt renders");
    let (metrics_x, metrics_y) = find("lines 2, duration 1.2s").expect("metrics render");
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
fn selected_tool_preview_reveals_one_wrapped_row_per_tick_without_touching_transcript_cache() {
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
    assert!(!text.contains("echo ok"));
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

    let fade_due = state
        .preview
        .reveal_deadline()
        .expect("first row fade is active");
    assert!(state
        .preview
        .tick_reveal(fade_due, state.config.preview_lines_per_second.get()));
    let final_fade_due = state
        .preview
        .reveal_deadline()
        .expect("first row final fade is active");
    assert!(state
        .preview
        .tick_reveal(final_fade_due, state.config.preview_lines_per_second.get()));
    let due = state
        .preview
        .reveal_deadline()
        .expect("next Preview row is queued");
    assert!(state
        .preview
        .tick_reveal(due, state.config.preview_lines_per_second.get()));
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
    assert!(text.contains("echo ok"));
    assert!(state.render.transcript_cache.valid);
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
    let initially_selected = state.reading.as_ref().unwrap().block_cursor.clone();
    assert_eq!(
        state.preview.policy,
        crate::preview::PreviewPolicy::FollowReadingCursor
    );
    assert!(state.reading_copy_text().unwrap().starts_with("block"));

    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
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
    assert_eq!(input.buf, original.buf);
    assert_eq!(input.cursor, original.cursor);
    assert!(input.multiline);
    assert_eq!(
        state.preview.policy,
        crate::preview::PreviewPolicy::FollowLatestBlock
    );
}

#[test]
fn reading_item_highlight_is_local_and_preview_takes_precedence() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let source = "`chip` [first](https://one.example) and [second](https://two.example)";
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "items"),
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
    let mut terminal = Terminal::new(TestBackend::new(70, 14)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    assert!(state.enter_reading(&input, &mut scroll, 9));
    assert!(state.enter_reading_items());
    let block_copy = state.reading_copy_text().unwrap();
    let item_id = state
        .reading
        .as_ref()
        .unwrap()
        .item_cursor
        .clone()
        .expect("item cursor");
    assert_eq!(block_copy, source);
    assert_eq!(state.preview.target.as_ref().unwrap().id, item_id.0);
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let highlighted = buffer
        .content()
        .iter()
        .filter(|cell| cell.bg == theme.selection)
        .count();
    assert!(highlighted > 0 && highlighted < 62);
    assert!(buffer
        .content()
        .iter()
        .any(|cell| cell.bg == theme.markdown.inline_code.bg.unwrap()));
    assert!(state.leave_reading_items());
    assert!(state.reading.as_ref().unwrap().item_cursor.is_none());
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
fn preview_markdown_code_uses_weak_syntax_and_code_block_bg() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_preview_only(&mut state);
    state.preview.state = PreviewState::Ready(PreviewContent::Markdown(
        "```rust\nfn main() {}\n```".into(),
    ));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut terminal = Terminal::new(TestBackend::new(70, 20)).unwrap();
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (x, y) = find_text(buffer, "fn main").expect("highlighted code body");
    assert_eq!(buffer[(x, y)].fg, theme.code_weak.keyword.fg);
    assert_eq!(
        buffer[(x, y)].bg,
        theme.markdown_weak.code_block_bg.bg.unwrap_or(Color::Reset)
    );
    assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
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

    theme = Theme::deepseek_e();
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
    assert!(content.contains("│    1 │ fn old"));
    assert!(content.contains("│    1 │ fn new"));
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
fn list_item_inline_code_paints_its_chip() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    force_message_only(&mut state);
    let source = "1. run `cargo fmt` now";
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "list-chip"),
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

    // The chip must paint its own foreground/background inside a list item,
    // just as it does in a paragraph.
    let buffer = terminal.backend().buffer();
    let chip_bg = theme
        .markdown
        .inline_code
        .bg
        .expect("ferra chip background");
    let cells = buffer
        .content()
        .iter()
        .filter(|cell| cell.bg == chip_bg)
        .collect::<Vec<_>>();
    let text = cells.iter().map(|cell| cell.symbol()).collect::<String>();
    assert_eq!(
        text, " cargo fmt ",
        "the chip covers the code text plus one padding cell each side"
    );
    assert!(
        cells
            .iter()
            .all(|cell| cell.fg == theme.markdown.inline_code.fg),
        "every chip cell keeps the inline-code foreground"
    );
    // The marker itself stays a marker: coral number, no chip background.
    let row = (0..12u16)
        .find(|y| {
            (0..40u16)
                .map(|x| buffer[(x, *y)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
                .contains("1.")
        })
        .expect("marker row painted");
    let marker_x = (0..40u16)
        .find(|x| buffer[(*x, row)].symbol() == "1")
        .expect("marker cell");
    assert_eq!(buffer[(marker_x, row)].fg, theme.coral);
    assert_eq!(buffer[(marker_x, row)].bg, Color::Reset);
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
    let queue = vec!["排队提示".to_string()];
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
        flat.contains("审批") && flat.contains("bash"),
        "approval card painted from overlays; screen:\n{text}"
    );
    assert!(
        flat.contains("排队提示"),
        "queued prompt strip painted from overlays; screen:\n{text}"
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
                &crate::SelectionFrame::default(),
            ));
        })
        .unwrap();
    let mut committed = rendered
        .expect("render returns selection geometry")
        .selection_frame;
    committed.set_epoch(1);

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
        &committed,
    );
    let copied = selection.handle(
        PointerEvent::PrimaryRelease {
            column: b_x,
            row: y,
        },
        &committed,
    );
    assert_eq!(
        copied.copy.as_deref(),
        Some(source),
        "coords=({a_x},{y})..({b_x},{y}), frame={committed:#?}"
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
fn mouse_selection_keeps_preview_and_transcript_ranges_independent() {
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
                &crate::SelectionFrame::default(),
            ));
        })
        .unwrap();
    let mut committed = rendered
        .expect("render returns split-pane geometry")
        .selection_frame;
    committed.set_epoch(2);
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
        &committed,
    );
    let preview_copy = selection.handle(
        PointerEvent::PrimaryRelease {
            column: preview_x + 6,
            row: preview_y,
        },
        &committed,
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
        &committed,
    );
    let copied = selection.handle(
        PointerEvent::PrimaryRelease {
            column: preview_x,
            row: preview_y,
        },
        &committed,
    );
    assert_eq!(
        copied.copy.as_deref(),
        Some("main text"),
        "cross-pane drag clamps to the starting Transcript surface"
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
    assert!(buffer[(main_x, main_y)]
        .modifier
        .contains(Modifier::REVERSED));
    assert!(!buffer[(preview_x, preview_y)]
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
fn pane_separator_uses_the_theme_background_in_normal_mode() {
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
    let separator_x = 72;
    assert_eq!(buffer[(separator_x, 10)].symbol(), "│");
    assert_eq!(buffer[(separator_x, 10)].fg, theme.separator.bar.fg);
    assert_eq!(buffer[(separator_x, 10)].bg, Color::Rgb(1, 2, 3));
    assert_eq!(buffer[(separator_x, 0)].symbol(), " ");
}

#[test]
fn pane_separator_drag_paints_only_bounded_theme_placeholders() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.transcript.append(
        DisplayItem::Block(crate::display::TranscriptBlock {
            id: DisplayId::correlated("assistant", "resize-release"),
            unit: None,
            content: "restored content".into(),
            format: crate::display::TranscriptFormat::Plain,
            tone: DisplayTone::Normal,
            copy_source: "restored content".into(),
            streaming: false,
        }),
        None,
    );
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut resize = crate::interaction::PaneResizeState::default();
    resize.begin(72, state.config.message_pane_percent, false);
    resize.update(54, 120);
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal
        .draw(|frame| {
            render_with_cursor_and_selection(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                RenderOverlays {
                    pane_resize: resize,
                    ..overlays()
                },
                &MouseSelection::default(),
                &crate::SelectionFrame::default(),
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let placeholder_bg = theme
        .separator
        .placeholder
        .bg
        .expect("Ferra placeholder defines its background");
    let line_bg = theme
        .separator
        .line
        .bg
        .expect("Ferra line defines its background");
    let bar_bg = theme
        .separator
        .bar
        .bg
        .expect("Ferra bar defines its background");
    assert_eq!(
        buffer[(2, 1)].bg,
        placeholder_bg,
        "message placeholder has its themed margin fill"
    );
    assert_eq!(
        buffer[(56, 1)].bg,
        placeholder_bg,
        "preview placeholder has its themed margin fill"
    );
    assert_eq!(buffer[(54, 0)].symbol(), "│", "drag guide is full height");
    assert_eq!(
        buffer[(54, 0)].bg,
        line_bg,
        "drag guide uses the theme background"
    );
    assert_eq!(buffer[(54, 10)].symbol(), "┃", "drag grip is thicker");
    assert_eq!(
        buffer[(54, 10)].bg,
        bar_bg,
        "drag grip uses the theme background"
    );
    assert!(buffer.content().iter().any(|cell| cell.symbol() == "消"));
    assert!(buffer.content().iter().any(|cell| cell.symbol() == "预"));
    assert!(find_text(buffer, "padding =").is_none());

    let transcript_work = state.render.transcript_cache.take_work_stats();
    let preview_work = state.preview.take_work_stats();
    assert_eq!(transcript_work.rebuilds, 0);
    assert_eq!(transcript_work.patches, 0);
    assert_eq!(preview_work.rebuilds, 0);
    assert_eq!(preview_work.patches, 0);
    assert_eq!(preview_work.materialized_rows, 0);

    let committed = resize.finish().expect("the test drag is still captured");
    state.config.message_pane_percent = committed.pending_percent;
    terminal
        .draw(|frame| {
            render(frame, &mut state, &input, &mut scroll, &theme, overlays());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(find_text(buffer, "消息栏").is_none());
    assert!(find_text(buffer, "restored content").is_some());
    let transcript_work = state.render.transcript_cache.take_work_stats();
    let preview_work = state.preview.take_work_stats();
    assert!(transcript_work.rebuilds <= 1);
    assert!(transcript_work.patches <= 1);
    assert!(preview_work.rebuilds <= 1);
    assert!(preview_work.patches <= 1);
    assert!(preview_work.materialized_rows <= 1);
}

#[test]
fn collapsed_separator_drag_paints_one_theme_message_placeholder() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let theme = Theme::ferra();
    let mut resize = crate::interaction::PaneResizeState::default();
    resize.begin(118, state.config.message_pane_percent, true);
    resize.update(118, 120);
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
                    pane_resize: resize,
                    ..overlays()
                },
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(2, 1)].bg,
        theme
            .separator
            .placeholder
            .bg
            .expect("Ferra placeholder background")
    );
    assert_eq!(buffer[(118, 0)].symbol(), "│");
    assert_eq!(
        buffer[(118, 0)].bg,
        theme.separator.line.bg.expect("Ferra line background")
    );
    assert_eq!(buffer[(118, 10)].symbol(), "┃");
    assert_eq!(
        buffer[(118, 10)].bg,
        theme.separator.bar.bg.expect("Ferra bar background")
    );
    assert!(buffer.content().iter().any(|cell| cell.symbol() == "消"));
    assert!(!buffer.content().iter().any(|cell| cell.symbol() == "预"));
}
