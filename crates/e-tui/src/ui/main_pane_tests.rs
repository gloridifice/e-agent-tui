use ratatui::{backend::TestBackend, layout::Position, style::Color, Terminal};

use crate::{
    app::TuiApp,
    display::{CardRole, ContentCard, DisplayId, DisplayItem, DisplayTone},
    input::InputState,
    preview::{
        PreviewContent, PreviewState, ToolMetrics, ToolPreview, ToolPreviewPrimary,
        ToolPreviewSecondary,
    },
    theme::Theme,
};

use super::*;

fn overlays() -> RenderOverlays<'static> {
    RenderOverlays {
        help_visible: false,
        toast: None,
        input_page: None,
        settings: None,
        login: None,
    }
}

#[test]
fn input_bar_box_grows_with_wrapped_rows_and_keeps_cursor_visible() {
    // 80 cols → split main 48 / preview 32 → page width 40 → inner 36 with
    // the default 2-column gutter. 200 chars wrap to 6 rows, so the box must
    // grow to the 3-row cap and the wrap window must follow the cursor: the
    // last three wrapped rows are visible and the IME anchor stays inside the
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
    // Box = 3 text rows + 2 padding = 5 rows at the bottom: y 16..20
    // (bottom stack: 5 + 1 gap + 1 status + 1 title = 8).
    assert_eq!(&row(17)[6..42], "x".repeat(36), "first visible wrapped row");
    assert_eq!(
        &row(18)[6..42],
        "x".repeat(36),
        "middle visible wrapped row"
    );
    assert_eq!(
        &row(19)[6..26],
        "x".repeat(20),
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
fn input_bar_multiline_wrapped_rows_keep_cursor_row_in_box() {
    // "a\n" + 200 y's: 7 display rows total, box capped at 3 text rows; the
    // window follows the cursor so the last three wrapped rows are shown.
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
    assert_eq!(&row(17)[6..42], "y".repeat(36));
    assert_eq!(&row(18)[6..42], "y".repeat(36));
    assert_eq!(
        &row(19)[6..26],
        "y".repeat(20),
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
    // 100 chars → 3 wrapped rows at inner 36 → the whole content is visible
    // from the top; the cursor row is the last one.
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
    assert_eq!(&row(17)[6..42], "z".repeat(36), "row 1 from the top");
    assert_eq!(&row(18)[6..42], "z".repeat(36), "row 2 from the top");
    assert_eq!(&row(19)[6..34], "z".repeat(28), "row 3 from the top");
    assert_eq!(anchor, Some(Position::new(34, 19)));
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
    assert_eq!(input_rows(&input, 40, 2), INPUT_MAX_ROWS, "cap at 3 rows");
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
    // 7 `aaaa` words fill the 36-column input content width; the following
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
        &row(18)[6..40],
        "aaaa aaaa aaaa aaaa aaaa aaaa aaaa",
        "first wrapped row keeps the seven fitting words"
    );
    assert_eq!(
        &row(19)[6..20],
        "aaaa aaaa aaaa",
        "second wrapped row starts with the next whole word"
    );
    assert_eq!(
        anchor,
        Some(Position::new(40, 18)),
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

    assert_eq!(anchor, Some(Position::new(6, 35)));
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
    assert_eq!(buffer[(4, 38)].bg, Color::Reset);
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
        incremental.render.transcript_cache.tail_len,
        incremental.render.transcript_cache.lines.len(),
        "the sole message owns the complete cached suffix including its gap"
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
fn extracted_main_pane_keeps_card_background_and_copy_provenance() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
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
    assert_eq!(label(4), '提', "label glyph starts the first row");
    assert_eq!(label(6), '示', "label glyph on the first row");
    assert_eq!(label(8), '词', "label glyph on the first row");
    assert_eq!(label(10), '注', "label glyph on the first row");
    assert_eq!(label(12), '入', "label glyph on the first row");
    assert_eq!(
        buffer[(4, 0)].fg,
        theme.activity.label.fg,
        "label uses the activity label tone (umber)"
    );
    assert_eq!(
        buffer[(15, 0)].fg,
        theme.activity.detail.fg,
        "content uses the activity detail tone (bark)"
    );
    assert_eq!(buffer[(4, 0)].bg, Color::Reset, "no card background");
    assert_eq!(buffer[(15, 0)].bg, Color::Reset, "no card background");
    // Capped at two wrapped rows with an explicit ellipsis marker on row 2.
    assert_eq!(
        row_text(1).trim(),
        format!("{}…", "x".repeat(31)),
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
    assert_eq!(buffer[(4, 0)].symbol(), "提");
    assert_eq!(buffer[(6, 0)].symbol(), "示");
    assert_eq!(buffer[(8, 0)].symbol(), "词");
    assert_eq!(buffer[(10, 0)].symbol(), "注");
    assert_eq!(buffer[(12, 0)].symbol(), "入");
    assert!(row_text(0).contains("short context"));
    assert!(row_text(1).trim().is_empty(), "one row then the gap");
    assert_eq!(buffer[(4, 0)].fg, theme.activity.label.fg);
    assert_eq!(buffer[(15, 0)].fg, theme.activity.detail.fg);
    assert_eq!(buffer[(4, 0)].bg, Color::Reset, "no card background");
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
    // Border, title, and pane background are removed; content is vertically
    // centered in the 30-row pane starting at the first padded column.
    assert_ne!(buffer[(72, 0)].symbol(), "┌");
    assert_eq!(buffer[(73, 13)].symbol(), "#");
    let preview_text = (0..30)
        .flat_map(|y| (72..120).map(move |x| buffer[(x, y)].symbol()))
        .collect::<String>();
    assert!(preview_text.contains("Preview source"));
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
    // Reasoning preview uses the muted (Bark) tone: the first content cell of
    // the centered single row carries `surface.muted_text`'s foreground.
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(73, 14)].fg,
        theme.surface.muted_text.fg,
        "reasoning preview must use the Bark/muted tone"
    );
}

#[test]
fn tool_preview_renders_header_primary_and_secondary_with_ferra_semantics() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.preview.fullscreen = true;
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
fn narrow_fullscreen_preview_renders_loading_and_error_states() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.preview.fullscreen = true;
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
    state.preview.fullscreen = true;
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
    state.preview.fullscreen = true;
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
    state.preview.fullscreen = true;
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
fn reasoning_preview_renders_markdown_with_forced_bark_foreground() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
    state.preview.fullscreen = true;
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
        bold.fg, theme.surface.muted_text.fg,
        "bold span foreground forced to Bark"
    );
    assert!(
        bold.modifier.contains(ratatui::style::Modifier::BOLD),
        "bold modifier preserved"
    );
    let code = &terminal.backend().buffer()[(13, 9)];
    assert_eq!(
        code.fg, theme.surface.muted_text.fg,
        "inline code foreground forced to Bark"
    );
    assert_eq!(
        code.bg,
        theme.markdown.inline_code.bg.unwrap_or(Color::Reset),
        "inline code chip background preserved"
    );
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
fn list_item_inline_code_paints_its_chip() {
    let mut state = TuiApp::default();
    state.config.resolved_theme = Theme::ferra();
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
