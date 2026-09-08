use super::*;
use crate::{
    i18n::{tr, tr_args, Language},
    input::Suggestion,
    interaction::{PendingPrompt, PromptDelivery},
};

/// Right-hand blank gutter shared by every non-block accessory strip, so
/// long/truncated rows (including their `…` ellipsis) never run flush into
/// the last column. The input bar uses a configurable symmetric gutter and
/// the approval card uses `Padding::new(1, 1, 0, 0)`; this constant gives the
/// plain info/todo/queue strips the same breathing room.
const ACCESSORY_RIGHT_PAD: u16 = 1;

pub(super) fn render_info_accessory(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    label_key: &str,
    value: &str,
    theme: &Theme,
    language: Language,
) {
    if area.height == 0 {
        return;
    }
    let label = tr(language, label_key);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("  {label}: "),
                Style::default().fg(theme.selection).bg(theme.bg),
            ),
            Span::styled(
                trim_to_width(
                    value,
                    area.width.saturating_sub(
                        UnicodeWidthStr::width(label.as_str()) as u16 + 4 + ACCESSORY_RIGHT_PAD,
                    ) as usize,
                ),
                Style::default().fg(theme.dim).bg(theme.bg),
            ),
        ]))
        .style(Style::default().bg(theme.bg)),
        area,
    );
}

pub(super) fn render_todo(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    todos: &[(String, String)],
    theme: &Theme,
    language: Language,
) {
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);
    if area.height == 0 {
        return;
    }
    let mut rows = vec![Line::from(Span::styled(
        format!("  {}", tr(language, "accessory.todo")),
        Style::default()
            .fg(theme.selection)
            .bg(theme.bg)
            .add_modifier(Modifier::BOLD),
    ))];
    let visible = usize::from(area.height.saturating_sub(1));
    for (content, status) in todos.iter().take(visible) {
        let marker = match status.as_str() {
            "completed" => "✓",
            "in-progress" => "•",
            _ => "○",
        };
        rows.push(Line::from(Span::styled(
            format!(
                "  {marker} {}",
                trim_to_width(
                    content,
                    area.width.saturating_sub(4 + ACCESSORY_RIGHT_PAD) as usize
                )
            ),
            Style::default().fg(theme.dim).bg(theme.bg),
        )));
    }
    frame.render_widget(Paragraph::new(rows), area);
}

/// Pending-prompt queue strip above the input bar. Steering prompts are shown
/// first with `⌁`, followed by after-turn prompts with `○`; insertion order is
/// retained within each class and every prompt occupies exactly one row.
pub(super) fn render_queue(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    queue: &[PendingPrompt],
    visible: usize,
    theme: &Theme,
    language: Language,
) {
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);
    let truncated = queue.len() > visible;
    let shown = visible.saturating_sub(usize::from(truncated));
    let width = (area.width as usize).saturating_sub(4 + usize::from(ACCESSORY_RIGHT_PAD));
    let ordered = queue
        .iter()
        .filter(|item| item.delivery == PromptDelivery::Asap)
        .chain(
            queue
                .iter()
                .filter(|item| item.delivery == PromptDelivery::AfterTurn),
        );
    let mut rows: Vec<Line<'static>> = ordered
        .take(shown)
        .map(|item| {
            let marker = match item.delivery {
                PromptDelivery::Asap => "⌁",
                PromptDelivery::AfterTurn => "○",
            };
            let preview: String = item
                .prompt
                .display_text_in(language)
                .chars()
                .map(|ch| {
                    if ch.is_whitespace() || ch.is_control() {
                        ' '
                    } else {
                        ch
                    }
                })
                .collect();
            Line::from(Span::styled(
                format!("  {marker} {}", trim_to_width(&preview, width)),
                Style::default().fg(theme.dim).bg(theme.bg),
            ))
        })
        .collect();
    if truncated {
        rows.push(Line::from(Span::styled(
            format!(
                "  * {}",
                trim_to_width(
                    &tr_args(
                        language,
                        "accessory.queue_more",
                        &[("count", (queue.len() - shown).to_string())],
                    ),
                    width,
                )
            ),
            Style::default().fg(theme.dim).bg(theme.bg),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(rows)), area);
}

/// Slash-command suggestion popup, anchored immediately above the input bar.
/// The popup uses the same ruled chrome as the composer: a full-width divider,
/// a compact four-row window, and a directional overflow marker. Commands and
/// descriptions keep separate tones so the catalog reads as a two-column list.
pub(super) fn render_suggest(
    frame: &mut Frame,
    suggest: &Suggestion,
    input_area: ratatui::layout::Rect,
    theme: &Theme,
) {
    // Keep the catalog compact even when a host contributes a large command
    // registry. The selected row still scrolls through the complete list.
    const MAX_VISIBLE_ROWS: usize = 6;
    if input_area.width == 0 || input_area.y == 0 || suggest.matches.is_empty() {
        return;
    }

    // Two ruled rows surround the list. If the list overflows, reserve one
    // more row for the direction marker; on a very short terminal the list
    // can still render without that marker rather than escaping the viewport.
    let room = usize::from(input_area.y);
    let without_marker = room.saturating_sub(2);
    let with_marker = room.saturating_sub(3);
    let mut visible_rows = suggest
        .matches
        .len()
        .min(MAX_VISIBLE_ROWS)
        .min(without_marker);
    let has_overflow = visible_rows < suggest.matches.len();
    if has_overflow && with_marker > 0 {
        visible_rows = visible_rows.min(with_marker);
    }
    if visible_rows == 0 {
        return;
    }

    let start = suggest
        .sel
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(suggest.matches.len().saturating_sub(visible_rows));
    let end = start + visible_rows;
    let above = start > 0;
    let below = end < suggest.matches.len();
    let marker = match (above, below) {
        (true, true) => Some("  ↕"),
        (true, false) => Some("  ↑"),
        (false, true) => Some("  ↓"),
        (false, false) => None,
    };
    let marker_rows = usize::from(marker.is_some());
    let height = visible_rows + marker_rows + 2;
    let rect = ratatui::layout::Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height as u16),
        width: input_area.width,
        height: height as u16,
    };
    // The popup floats over transcript rows. Clear first so short text rows
    // do not leak the transcript through the transparent completion area.
    frame.render_widget(ratatui::widgets::Clear, rect);
    let width = usize::from(rect.width);
    let file_paths = suggest.kind == crate::input::SuggestionKind::Files;
    let labels = if file_paths {
        &suggest.descriptions
    } else {
        &suggest.matches
    };
    let command_width = labels
        .iter()
        .map(|command| UnicodeWidthStr::width(command.as_str()))
        .max()
        .unwrap_or_default();
    let description_column = width.min(command_width.saturating_add(4));

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(height);
    lines.push(Line::from(Span::styled(
        "─".repeat(width),
        theme.input.hint.style(),
    )));
    for (index, command) in labels.iter().enumerate().skip(start).take(visible_rows) {
        let selected = index == suggest.sel;
        // Keep the fill-in command intact. On narrow rows the description
        // yields all remaining space before the command is clipped by the
        // terminal viewport.
        let command_text = command.clone();
        let command_used = UnicodeWidthStr::width(command_text.as_str());
        let gap = description_column.saturating_sub(2 + command_used);
        let description = if file_paths {
            ""
        } else {
            suggest
                .descriptions
                .get(index)
                .map(String::as_str)
                .unwrap_or("")
        };
        let description_width = width.saturating_sub(description_column);
        let description_text = if description_width == 0 {
            String::new()
        } else {
            trim_to_width(description, description_width)
        };
        let used = 2 + command_used + gap + UnicodeWidthStr::width(description_text.as_str());
        let tail = width.saturating_sub(used);
        let command_style = match suggest.sources.get(index).copied() {
            Some(CommandSource::Integrated) => Style::default().fg(theme.overlay.text.fg),
            _ => Style::default().fg(theme.markdown.heading2.fg),
        };
        let command_style = if selected {
            command_style.add_modifier(Modifier::BOLD)
        } else {
            command_style
        };
        let description_style = theme.activity.detail.style();
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(command_text, command_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(description_text, description_style),
            Span::raw(" ".repeat(tail)),
        ]));
    }
    if let Some(marker) = marker {
        let marker_width = UnicodeWidthStr::width(marker);
        lines.push(Line::from(Span::styled(
            format!("{marker}{}", " ".repeat(width.saturating_sub(marker_width))),
            theme.input.hint.style(),
        )));
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(width),
        theme.input.hint.style(),
    )));
    frame.render_widget(Paragraph::new(Text::from(lines)), rect);
    super::input::render_rule(frame, rect, rect.y, theme);
    super::input::render_rule(frame, rect, rect.bottom().saturating_sub(1), theme);
}

/// Pending approval card: fixed above the input bar (design §4.4).
pub(super) fn render_approval(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    approval: &crate::interaction::ApprovalCard,
    theme: &Theme,
    config: &crate::Config,
) {
    let language = config.language;
    use crate::key_mapping::{Action, Scope};
    let title = Line::from(vec![
        Span::styled(
            format!("⚠ {}", tr(language, "accessory.approval.title")),
            Style::default()
                .fg(theme.running)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(theme.dim)),
        Span::styled(approval.tool_name.clone(), Style::default().fg(theme.fg)),
    ]);
    let mut rows = vec![title];
    if !approval.reason.is_empty() {
        rows.push(Line::from(Span::styled(
            approval.reason.chars().take(100).collect::<String>(),
            Style::default().fg(theme.dim),
        )));
    }
    rows.push(Line::from(vec![
        Span::styled(
            format!(
                "[{}] {}",
                config.key_mapping.label(Scope::Approval, Action::Allow),
                tr(language, "accessory.approval.allow")
            ),
            Style::default().fg(theme.ok),
        ),
        Span::styled("   ", Style::default().fg(theme.dim)),
        Span::styled(
            format!(
                "[{}] {}",
                config.key_mapping.label(Scope::Approval, Action::Deny),
                tr(language, "accessory.approval.deny")
            ),
            Style::default().fg(theme.err),
        ),
    ]));
    let block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(theme.running))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(Text::from(rows)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    /// Render one accessory strip into a width×height test frame and return
    /// every row as a string of cell symbols.
    fn strip_rows<F>(width: u16, height: u16, f: F) -> Vec<String>
    where
        F: FnOnce(&mut Frame, Rect, &Theme),
    {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::ferra();
        terminal
            .draw(|frame| f(frame, Rect::new(0, 0, width, height), &theme))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
            })
            .collect()
    }

    fn assert_last_column_blank(rows: &[String], y: usize, width: usize) {
        let last = width - 1;
        assert_ne!(
            rows[y].chars().nth(last),
            Some('…'),
            "ellipsis must not touch the right edge"
        );
        assert_eq!(
            rows[y].chars().nth(last),
            Some(' '),
            "right padding column must be blank"
        );
    }

    /// Long rows truncate to the shared right gutter instead of running flush
    /// into the last column (regression: ellipsis used to land on the edge).
    #[test]
    fn accessory_rows_reserve_right_padding_when_truncated() {
        let width = 12usize;

        let queue = strip_rows(width as u16, 1, |frame, area, theme| {
            render_queue(
                frame,
                area,
                &[PendingPrompt::new(
                    "abcdefghijklmnop".into(),
                    PromptDelivery::Asap,
                )],
                1,
                theme,
                Language::English,
            );
        });
        assert_last_column_blank(&queue, 0, width);
        assert!(
            queue[0].contains('…'),
            "long prompt truncates: {:?}",
            queue[0]
        );

        let todo = strip_rows(width as u16, 2, |frame, area, theme| {
            render_todo(
                frame,
                area,
                &[("abcdefghijklmnop".to_string(), "pending".to_string())],
                theme,
                Language::English,
            );
        });
        assert_last_column_blank(&todo, 1, width);
        assert!(todo[1].contains('…'), "long todo truncates: {:?}", todo[1]);

        let info = strip_rows(width as u16, 1, |frame, area, theme| {
            render_info_accessory(
                frame,
                area,
                "accessory.goal",
                "abcdefghijklmnop",
                theme,
                Language::English,
            );
        });
        assert_last_column_blank(&info, 0, width);
        assert!(info[0].contains('…'), "long info truncates: {:?}", info[0]);
    }

    #[test]
    fn suggestion_keeps_the_command_and_omits_the_description_when_narrow() {
        let backend = TestBackend::new(8, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::ferra();
        let suggestion = Suggestion {
            query: "/model".into(),
            sel: 0,
            matches: vec!["/model".into()],
            descriptions: vec!["Select a model".into()],
            sources: vec![CommandSource::Builtin],
            kind: crate::input::SuggestionKind::Commands,
        };

        terminal
            .draw(|frame| render_suggest(frame, &suggestion, Rect::new(0, 4, 8, 1), &theme))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = (0..8).map(|x| buffer[(x, 2)].symbol()).collect::<String>();

        assert_eq!(row, "  /model");
        assert!(!row.contains('…'));
        assert!(!row.contains("Select"));
    }

    #[test]
    fn path_suggestion_displays_only_the_entry_name() {
        let backend = TestBackend::new(30, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let suggestion = Suggestion {
            query: "Please read @foo/".into(),
            sel: 0,
            matches: vec!["Please read @foo/a.rs".into()],
            descriptions: vec!["a.rs".into()],
            sources: vec![CommandSource::Builtin],
            kind: crate::input::SuggestionKind::Files,
        };
        terminal
            .draw(|frame| {
                render_suggest(frame, &suggestion, Rect::new(0, 4, 30, 1), &Theme::ferra())
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = (0..30).map(|x| buffer[(x, 2)].symbol()).collect::<String>();
        assert_eq!(row.trim(), "a.rs");
    }

    #[test]
    fn queue_renders_asap_before_after_turn_without_losing_class_order() {
        let queue = [
            PendingPrompt::new("a".into(), PromptDelivery::Asap),
            PendingPrompt::new("b".into(), PromptDelivery::AfterTurn),
            PendingPrompt::new("c".into(), PromptDelivery::Asap),
            PendingPrompt::new("d".into(), PromptDelivery::AfterTurn),
        ];
        let rows = strip_rows(20, 4, |frame, area, theme| {
            render_queue(frame, area, &queue, 4, theme, Language::English);
        });
        assert!(rows[0].contains("⌁ a"));
        assert!(rows[1].contains("⌁ c"));
        assert!(rows[2].contains("○ b"));
        assert!(rows[3].contains("○ d"));
    }

    #[test]
    fn queue_flattens_multiline_backend_previews_to_one_row() {
        let mut queue = crate::interaction::PendingPromptQueue::default();
        queue.update_remote(vec!["first\nsecond\tthird".into()]);
        queue.push("waiting".into(), PromptDelivery::AfterTurn);
        let rows = strip_rows(40, 2, |frame, area, theme| {
            render_queue(frame, area, queue.entries(), 2, theme, Language::English);
        });
        assert!(rows[0].contains("⌁ first second third"));
        assert!(rows[1].contains("○ waiting"));
    }

    /// The pending-count summary row keeps the same right gutter.
    #[test]
    fn queue_summary_row_respects_right_padding() {
        let width = 12usize;
        let queue = strip_rows(width as u16, 1, |frame, area, theme| {
            render_queue(
                frame,
                area,
                &[
                    PendingPrompt::new("a".into(), PromptDelivery::Asap),
                    PendingPrompt::new("b".into(), PromptDelivery::AfterTurn),
                    PendingPrompt::new("c".into(), PromptDelivery::Asap),
                ],
                1,
                theme,
                Language::English,
            );
        });
        assert_last_column_blank(&queue, 0, width);
    }
}
