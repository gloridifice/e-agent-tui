use ratatui::{backend::TestBackend, layout::Position, style::Color, Terminal};

use crate::{
    app::TuiApp,
    display::{CardRole, ContentCard, DisplayId, DisplayItem, DisplayTone},
    input::InputState,
    preview::{PreviewContent, PreviewState},
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
