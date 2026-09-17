//! Execution-history ranking document rendering.

use std::collections::BTreeMap;

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    execution_history::{
        ExecutionCall, ExecutionEvent, ExecutionOutcome, HistoryMessageKind, OperationKind,
        OperationSummary,
    },
    history_page::{HistoryLoadState, HistoryPage, HistoryView},
    key_mapping::{Action, KeyMapping, Scope},
    theme::{Theme, ThemeStyle},
    ui::component::command::{self, CommandColors},
    wrap::ellipsize_line,
};

#[derive(Clone)]
struct Run {
    x: usize,
    text: String,
    style: Style,
}

type Row = Vec<Run>;

fn put(row: &mut Row, x: usize, text: impl Into<String>, style: ThemeStyle) {
    row.push(Run {
        x,
        text: text.into(),
        style: style.style(),
    });
}

fn blank() -> Row {
    Vec::new()
}

fn operation_style(theme: &Theme, kind: OperationKind) -> ThemeStyle {
    match kind {
        OperationKind::Model => theme.history.operation.model,
        OperationKind::Read => theme.history.operation.read,
        OperationKind::Edit => theme.history.operation.edit,
        OperationKind::Command => theme.history.operation.bash,
        OperationKind::Search => theme.history.operation.search,
        OperationKind::Other => theme.history.operation.other,
    }
}

fn operation_label(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Model => "model",
        OperationKind::Read => "read",
        OperationKind::Edit => "edit",
        OperationKind::Command => "bash",
        OperationKind::Search => "search",
        OperationKind::Other => "other",
    }
}

fn summary(call: &ExecutionCall) -> String {
    match &call.operation.summary {
        OperationSummary::Identity => {
            if call.operation.kind == OperationKind::Model {
                "model request".into()
            } else {
                call.operation.name.clone()
            }
        }
        OperationSummary::Command { command } => command.clone(),
        OperationSummary::Paths { paths } => paths.join(", "),
        OperationSummary::Search { query, path } => path
            .as_ref()
            .map_or_else(|| query.clone(), |path| format!("{query} at {path}")),
    }
}

fn put_summary(row: &mut Row, mut x: usize, call: &ExecutionCall, width: usize, theme: &Theme) {
    let OperationSummary::Command { command } = &call.operation.summary else {
        put(
            row,
            x,
            clip_width(&summary(call), width),
            theme.history.text,
        );
        return;
    };
    let lines = command::highlight(
        command,
        CommandColors {
            executable: theme.history.operation.bash.fg,
            argument: theme.history.text.fg,
            operator: theme.history.metadata.fg,
        },
    );
    let mut spans = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                " ",
                Style::default().fg(theme.history.text.fg),
            ));
        }
        spans.extend(line.spans);
    }
    for span in ellipsize_line(Line::from(spans), width).spans {
        let columns = span.width();
        row.push(Run {
            x,
            text: span.content.into_owned(),
            style: span.style,
        });
        x += columns;
    }
}

fn clock(unix_ms: u64, precise: bool) -> String {
    let day = unix_ms % 86_400_000;
    let hour = day / 3_600_000;
    let minute = day / 60_000 % 60;
    let second = day / 1_000 % 60;
    if precise {
        format!("{hour:02}:{minute:02}:{second:02}.{:03}", day % 1_000)
    } else {
        format!("{hour:02}:{minute:02}:{second:02}")
    }
}

fn elapsed(call: &ExecutionCall) -> String {
    let Some(duration) = call.finish.as_ref().and_then(|finish| finish.duration) else {
        return "--".into();
    };
    if duration.duration_ms < 1_000 {
        format!("{}ms", duration.duration_ms)
    } else {
        format!("{:.2}s", duration.duration_ms as f64 / 1_000.0)
    }
}

fn clip_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width == 1 {
        return "…".into();
    }
    let mut output = String::new();
    let mut used = 0;
    for character in text.chars() {
        let next = character.width().unwrap_or(0);
        if used + next > width - 1 {
            break;
        }
        output.push(character);
        used += next;
    }
    output.push('…');
    output
}

fn duration_styles(calls: &[&ExecutionCall], theme: &Theme) -> BTreeMap<(String, u64), ThemeStyle> {
    let mut ranked: Vec<_> = calls
        .iter()
        .filter(|call| {
            call.finish
                .as_ref()
                .and_then(|finish| finish.duration)
                .is_some()
        })
        .copied()
        .collect();
    ranked.sort_by(|left, right| {
        let left_ms = left
            .finish
            .as_ref()
            .and_then(|finish| finish.duration)
            .unwrap()
            .duration_ms;
        let right_ms = right
            .finish
            .as_ref()
            .and_then(|finish| finish.duration)
            .unwrap()
            .duration_ms;
        right_ms
            .cmp(&left_ms)
            .then_with(|| left.sequence.cmp(&right.sequence))
    });
    ranked
        .into_iter()
        .enumerate()
        .map(|(rank, call)| {
            let style = match rank {
                0 => theme.history.duration.highest,
                1 => theme.history.duration.second,
                2..=4 => theme.history.duration.top_five,
                _ => theme.history.duration.remaining,
            };
            ((call.run_id.clone(), call.sequence), style)
        })
        .collect()
}

fn record_rows(width: usize, calls: &[&ExecutionCall], theme: &Theme) -> Vec<Row> {
    let duration_styles = duration_styles(calls, theme);
    let mut rows = Vec::new();
    let mut header = Vec::new();
    if width >= 80 {
        put(
            &mut header,
            2,
            "#   started       operation / command",
            theme.history.metadata,
        );
        put(
            &mut header,
            width.saturating_sub(31),
            "lines      elapsed   result",
            theme.history.metadata,
        );
    } else {
        put(
            &mut header,
            2,
            "operation / command · start · elapsed · result",
            theme.history.metadata,
        );
    }
    rows.push(header);
    rows.push(blank());
    for (rank, call) in calls.iter().enumerate() {
        let mut row = Vec::new();
        let operation = operation_style(theme, call.operation.kind);
        let duration = call.finish.as_ref().and_then(|finish| finish.duration);
        let duration_style = duration.map_or(theme.history.duration.unknown, |_| {
            duration_styles
                .get(&(call.run_id.clone(), call.sequence))
                .copied()
                .unwrap_or(theme.history.duration.remaining)
        });
        let (result, result_style) = match call.finish.as_ref().map(|finish| finish.outcome) {
            Some(ExecutionOutcome::Success) => ("✓", theme.working_status.success),
            Some(ExecutionOutcome::Failure) => ("✗", theme.working_status.failure),
            Some(ExecutionOutcome::Cancelled) => ("⊘", theme.working_status.cancelled),
            None => ("?", theme.working_status.running),
        };
        let lines = call
            .finish
            .as_ref()
            .and_then(|finish| finish.output_lines)
            .map_or_else(
                || "--".into(),
                |lines| format!("{}{}", lines.count, if lines.truncated { "+" } else { "" }),
            );
        if width >= 80 {
            put(
                &mut row,
                2,
                format!("{:02}", rank + 1),
                theme.history.metadata,
            );
            put(
                &mut row,
                6,
                clock(call.start_unix_ms, true),
                theme.history.metadata,
            );
            put(
                &mut row,
                20,
                operation_label(call.operation.kind),
                operation,
            );
            let command_width = width.saturating_sub(64).max(8);
            put_summary(&mut row, 28, call, command_width, theme);
            put(
                &mut row,
                width.saturating_sub(32),
                format!("{lines:>6}"),
                theme.history.metadata,
            );
            put(
                &mut row,
                width.saturating_sub(24),
                format!("{:>9}", elapsed(call)),
                duration_style,
            );
            put(&mut row, width.saturating_sub(9), result, result_style);
        } else {
            put(&mut row, 2, operation_label(call.operation.kind), operation);
            put_summary(&mut row, 10, call, width.saturating_sub(12), theme);
            rows.push(row);
            row = Vec::new();
            put(
                &mut row,
                2,
                clock(call.start_unix_ms, true),
                theme.history.metadata,
            );
            put(&mut row, 16, elapsed(call), duration_style);
            put(&mut row, 27, result, result_style);
            put(&mut row, 30, lines, theme.history.metadata);
        }
        rows.push(row);
    }
    rows
}

fn ready_document(width: usize, page: &HistoryPage, theme: &Theme) -> Vec<Row> {
    let mut heading = Vec::new();
    put(
        &mut heading,
        2,
        format!(
            "Top 50 · elapsed descending · {} measured calls",
            page.calls.len()
        ),
        theme.history.heading,
    );
    let mut rows = vec![heading, blank()];
    let mut legend = Vec::new();
    let mut x = 2;
    for kind in [
        OperationKind::Model,
        OperationKind::Read,
        OperationKind::Edit,
        OperationKind::Command,
        OperationKind::Search,
    ] {
        let label = format!("■ {}  ", operation_label(kind));
        put(&mut legend, x, &label, operation_style(theme, kind));
        x += label.width();
    }
    rows.push(legend);
    let mut key = Vec::new();
    put(&mut key, 2, "lines+: truncated", theme.history.metadata);
    rows.push(key);
    rows.push(blank());
    let references: Vec<_> = page.calls.iter().collect();
    rows.extend(record_rows(width, &references, theme));
    rows
}

const TIMELINE_ROW_MS: u64 = 5_000;

#[derive(Debug, Clone, Default)]
struct UsageSummary {
    tokens: u64,
    known_cost_usd_nanos: u64,
    missing_cost: bool,
    samples: usize,
}

impl UsageSummary {
    fn add(&mut self, tokens: u64, cost_usd_nanos: Option<u64>) {
        self.tokens = self.tokens.saturating_add(tokens);
        self.samples = self.samples.saturating_add(1);
        if let Some(cost) = cost_usd_nanos {
            self.known_cost_usd_nanos = self.known_cost_usd_nanos.saturating_add(cost);
        } else if tokens > 0 {
            self.missing_cost = true;
        }
    }
}

#[derive(Debug, Clone)]
struct TimelineItem {
    label: &'static str,
    kind: HistoryMessageKind,
    model: Option<String>,
    turn: Option<(String, String)>,
}

#[derive(Debug, Clone, Default)]
struct TimelineBucket {
    items: Vec<TimelineItem>,
}

struct TimelineDocument {
    header: Vec<Row>,
    buckets: BTreeMap<usize, TimelineBucket>,
    row_count: usize,
    origin_ms: u64,
    turn_usage: BTreeMap<(String, String), UsageSummary>,
}

fn model_text(model: &crate::execution_history::ModelIdentity) -> String {
    format!("{}/{}", model.provider, model.model)
}

fn comma_number(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

fn token_total(summary: &UsageSummary) -> String {
    if summary.samples == 0 {
        "unknown".into()
    } else {
        comma_number(summary.tokens)
    }
}

fn price(summary: &UsageSummary) -> String {
    if summary.samples == 0 {
        return "unknown".into();
    }
    if summary.missing_cost && summary.known_cost_usd_nanos == 0 {
        return "unknown".into();
    }
    let amount = summary.known_cost_usd_nanos as f64 / 1_000_000_000.0;
    if summary.missing_cost {
        format!("${amount:.4} + ?")
    } else {
        format!("${amount:.4}")
    }
}

fn message_label(kind: HistoryMessageKind) -> &'static str {
    match kind {
        HistoryMessageKind::User => "USER",
        HistoryMessageKind::Reasoning => "THINK",
        HistoryMessageKind::Assistant => "ASSISTANT",
        HistoryMessageKind::ToolCall => "TOOL",
        HistoryMessageKind::ToolResult => "RESULT",
        HistoryMessageKind::ModelChange => "MODEL",
        HistoryMessageKind::AgentStop => "AGENT STOP",
    }
}

fn message_style(theme: &Theme, kind: HistoryMessageKind) -> ThemeStyle {
    match kind {
        HistoryMessageKind::User => theme.history.heading,
        HistoryMessageKind::Reasoning | HistoryMessageKind::ModelChange => {
            theme.history.operation.model
        }
        HistoryMessageKind::Assistant => theme.history.operation.read,
        HistoryMessageKind::ToolCall => theme.history.operation.bash,
        HistoryMessageKind::ToolResult => theme.history.operation.search,
        HistoryMessageKind::AgentStop => theme.working_status.success,
    }
}

fn timeline_document(width: usize, page: &HistoryPage, theme: &Theme) -> TimelineDocument {
    let mut total = UsageSummary::default();
    let mut models: BTreeMap<String, UsageSummary> = BTreeMap::new();
    let mut turns: BTreeMap<(String, String), bool> = BTreeMap::new();
    let mut turn_usage: BTreeMap<(String, String), UsageSummary> = BTreeMap::new();
    for record in &page.records {
        match &record.event {
            ExecutionEvent::MessageObserved {
                turn_id: Some(turn_id),
                kind: HistoryMessageKind::User,
                ..
            } => {
                turns
                    .entry((record.run_id.clone(), turn_id.clone()))
                    .or_default();
            }
            ExecutionEvent::TurnFinished { turn_id, .. } => {
                if let Some(closed) = turns.get_mut(&(record.run_id.clone(), turn_id.clone())) {
                    *closed = true;
                }
            }
            ExecutionEvent::UsageRecorded {
                turn_id,
                model,
                usage,
                cost_usd_nanos,
            } => {
                let tokens = usage.total();
                total.add(tokens, *cost_usd_nanos);
                models
                    .entry(model.as_ref().map_or_else(|| "unknown".into(), model_text))
                    .or_default()
                    .add(tokens, *cost_usd_nanos);
                if let Some(turn_id) = turn_id {
                    turn_usage
                        .entry((record.run_id.clone(), turn_id.clone()))
                        .or_default()
                        .add(tokens, *cost_usd_nanos);
                }
            }
            _ => {}
        }
    }

    let closed = turns.values().filter(|closed| **closed).count();
    let open = turns.len().saturating_sub(closed);
    let mut event_operations: BTreeMap<(String, String), OperationKind> = BTreeMap::new();
    let mut events = 0_usize;
    for record in &page.records {
        match &record.event {
            ExecutionEvent::MessageObserved { .. } | ExecutionEvent::ModelSelected { .. } => {
                events += 1;
            }
            ExecutionEvent::Started(operation) => {
                event_operations.insert(
                    (record.run_id.clone(), operation.call_id.clone()),
                    operation.kind,
                );
                events += 1;
            }
            ExecutionEvent::Finished(finish)
                if event_operations
                    .get(&(record.run_id.clone(), finish.call_id.clone()))
                    .is_some_and(|kind| *kind != OperationKind::Model) =>
            {
                events += 1;
            }
            _ => {}
        }
    }
    let mut header = Vec::new();
    if width >= 86 {
        let mut labels = Vec::new();
        put(&mut labels, 2, "TOTAL TOKENS", theme.history.metadata);
        put(&mut labels, 25, "TOTAL COST", theme.history.metadata);
        put(&mut labels, 48, "TURNS", theme.history.metadata);
        put(&mut labels, 69, "EVENTS", theme.history.metadata);
        header.push(labels);
        let mut values = Vec::new();
        put(&mut values, 2, token_total(&total), theme.history.heading);
        put(
            &mut values,
            25,
            price(&total),
            theme.history.duration.highest,
        );
        put(
            &mut values,
            48,
            format!("{closed} closed / {open} open"),
            theme.history.text,
        );
        put(&mut values, 69, events.to_string(), theme.history.text);
        header.push(values);
    } else {
        let mut tokens = Vec::new();
        put(&mut tokens, 2, "TOTAL TOKENS", theme.history.metadata);
        put(&mut tokens, 18, token_total(&total), theme.history.heading);
        put(&mut tokens, 38, "TOTAL COST", theme.history.metadata);
        put(
            &mut tokens,
            52,
            price(&total),
            theme.history.duration.highest,
        );
        header.push(tokens);
        let mut turns_row = Vec::new();
        put(&mut turns_row, 2, "TURNS", theme.history.metadata);
        put(
            &mut turns_row,
            10,
            format!("{closed} closed / {open} open"),
            theme.history.text,
        );
        put(&mut turns_row, 38, "EVENTS", theme.history.metadata);
        put(&mut turns_row, 48, events.to_string(), theme.history.text);
        header.push(turns_row);
    }
    header.push(blank());
    for (model, usage) in &models {
        let mut row = Vec::new();
        let token_x: usize = if width >= 86 { 48 } else { 38 };
        let cost_x: usize = if width >= 86 { 69 } else { 54 };
        put(
            &mut row,
            2,
            clip_width(model, token_x.saturating_sub(4)),
            theme.history.operation.model,
        );
        put(
            &mut row,
            token_x,
            format!("{} tok", comma_number(usage.tokens)),
            theme.history.text,
        );
        put(
            &mut row,
            cost_x,
            price(usage),
            theme.history.duration.remaining,
        );
        header.push(row);
    }
    for warning in &page.warnings {
        let mut row = Vec::new();
        put(&mut row, 2, format!("! {warning}"), theme.log.warning);
        header.push(row);
    }
    header.push(blank());

    let mut raw_items = Vec::new();
    let mut current_models: BTreeMap<String, String> = BTreeMap::new();
    let mut operations: BTreeMap<(String, String), (OperationKind, Option<String>)> =
        BTreeMap::new();
    for record in &page.records {
        match &record.event {
            ExecutionEvent::MessageObserved {
                turn_id,
                kind,
                model,
            } => {
                let model = model
                    .as_ref()
                    .map(model_text)
                    .or_else(|| current_models.get(&record.run_id).cloned());
                if let Some(model) = &model {
                    current_models.insert(record.run_id.clone(), model.clone());
                }
                raw_items.push((
                    record.time_unix_ms,
                    TimelineItem {
                        label: message_label(*kind),
                        kind: *kind,
                        model,
                        turn: turn_id
                            .as_ref()
                            .map(|turn| (record.run_id.clone(), turn.clone())),
                    },
                ));
            }
            ExecutionEvent::ModelSelected { model } => {
                let model = model_text(model);
                current_models.insert(record.run_id.clone(), model.clone());
                raw_items.push((
                    record.time_unix_ms,
                    TimelineItem {
                        label: "MODEL",
                        kind: HistoryMessageKind::ModelChange,
                        model: Some(model),
                        turn: None,
                    },
                ));
            }
            ExecutionEvent::Started(operation) => {
                operations.insert(
                    (record.run_id.clone(), operation.call_id.clone()),
                    (operation.kind, operation.turn_id.clone()),
                );
                let kind = if operation.kind == OperationKind::Model {
                    HistoryMessageKind::Reasoning
                } else {
                    HistoryMessageKind::ToolCall
                };
                raw_items.push((
                    record.time_unix_ms,
                    TimelineItem {
                        label: message_label(kind),
                        kind,
                        model: current_models.get(&record.run_id).cloned(),
                        turn: operation
                            .turn_id
                            .as_ref()
                            .map(|turn| (record.run_id.clone(), turn.clone())),
                    },
                ));
            }
            ExecutionEvent::Finished(finish) => {
                let Some((kind, turn_id)) =
                    operations.get(&(record.run_id.clone(), finish.call_id.clone()))
                else {
                    continue;
                };
                if *kind == OperationKind::Model {
                    continue;
                }
                raw_items.push((
                    record.time_unix_ms,
                    TimelineItem {
                        label: message_label(HistoryMessageKind::ToolResult),
                        kind: HistoryMessageKind::ToolResult,
                        model: current_models.get(&record.run_id).cloned(),
                        turn: turn_id
                            .as_ref()
                            .map(|turn| (record.run_id.clone(), turn.clone())),
                    },
                ));
            }
            _ => {}
        }
    }
    let Some(first_ms) = raw_items.iter().map(|(time, _)| *time).min() else {
        let mut unavailable = Vec::new();
        put(
            &mut unavailable,
            2,
            "Timeline data is unavailable for this legacy history.",
            theme.history.metadata,
        );
        header.push(unavailable);
        return TimelineDocument {
            header,
            buckets: BTreeMap::new(),
            row_count: 0,
            origin_ms: 0,
            turn_usage,
        };
    };
    let last_ms = raw_items
        .iter()
        .map(|(time, _)| *time)
        .max()
        .unwrap_or(first_ms);
    let origin_ms = first_ms / TIMELINE_ROW_MS * TIMELINE_ROW_MS;
    let row_count =
        usize::try_from((last_ms - origin_ms) / TIMELINE_ROW_MS + 1).unwrap_or(usize::MAX);
    let mut buckets: BTreeMap<usize, TimelineBucket> = BTreeMap::new();
    for (time, item) in raw_items {
        let index = usize::try_from((time - origin_ms) / TIMELINE_ROW_MS).unwrap_or(usize::MAX);
        buckets.entry(index).or_default().items.push(item);
    }
    TimelineDocument {
        header,
        buckets,
        row_count,
        origin_ms,
        turn_usage,
    }
}

fn timeline_row(index: usize, width: usize, document: &TimelineDocument, theme: &Theme) -> Row {
    let mut row = Vec::new();
    let time = document
        .origin_ms
        .saturating_add((index as u64).saturating_mul(TIMELINE_ROW_MS));
    let (time_x, model_x, model_width, axis_x, event_x, detail_x): (
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
    ) = if width >= 100 {
        (2, 12, 30, 44, 48, 72)
    } else if width >= 70 {
        (1, 10, 20, 32, 36, 54)
    } else {
        (1, usize::MAX, 0, 10, 14, 36)
    };
    put(&mut row, time_x, clock(time, false), theme.history.metadata);
    put(&mut row, axis_x, "│", theme.history.separator);
    let Some(bucket) = document.buckets.get(&index) else {
        return row;
    };
    let mut model_names = Vec::new();
    for item in &bucket.items {
        if let Some(model) = &item.model {
            if !model_names.contains(model) {
                model_names.push(model.clone());
            }
        }
    }
    if model_x != usize::MAX {
        put(
            &mut row,
            model_x,
            clip_width(&model_names.join(", "), model_width),
            theme.history.operation.model,
        );
    }
    let details = bucket
        .items
        .iter()
        .filter(|item| item.kind == HistoryMessageKind::AgentStop)
        .filter_map(|item| item.turn.as_ref())
        .filter_map(|turn| document.turn_usage.get(turn))
        .map(|usage| format!("{} tok  {}", comma_number(usage.tokens), price(usage)))
        .collect::<Vec<_>>()
        .join(" · ");
    let mut grouped: Vec<(HistoryMessageKind, &'static str, usize)> = Vec::new();
    for item in &bucket.items {
        if let Some((_, _, count)) = grouped.iter_mut().find(|(kind, _, _)| *kind == item.kind) {
            *count += 1;
        } else {
            grouped.push((item.kind, item.label, 1));
        }
    }
    let event_limit = if details.is_empty() {
        width
    } else {
        detail_x.saturating_sub(1)
    };
    let mut x = event_x;
    for (item_index, (kind, label, count)) in grouped.into_iter().enumerate() {
        if x >= event_limit {
            break;
        }
        if item_index > 0 {
            if x.saturating_add(3) >= event_limit {
                break;
            }
            put(&mut row, x, " · ", theme.history.separator);
            x += 3;
        }
        let label = if count > 1 {
            format!("{label}×{count}")
        } else {
            label.to_owned()
        };
        let label = clip_width(&label, event_limit.saturating_sub(x));
        put(&mut row, x, &label, message_style(theme, kind));
        x += label.width();
    }
    if !details.is_empty() && detail_x < width {
        put(
            &mut row,
            detail_x,
            clip_width(&details, width.saturating_sub(detail_x)),
            theme.history.duration.highest,
        );
    }
    row
}

fn render_rows(frame: &mut Frame, area: Rect, rows: impl Iterator<Item = Row>, width: usize) {
    let buffer = frame.buffer_mut();
    for (local_y, row) in rows.enumerate() {
        let y = area.y + local_y as u16;
        for run in row {
            if run.x >= width {
                continue;
            }
            buffer.set_stringn(
                area.x + run.x as u16,
                y,
                &run.text,
                width - run.x,
                run.style,
            );
        }
    }
}

fn render_timeline(frame: &mut Frame, area: Rect, page: &mut HistoryPage, theme: &Theme) {
    let width = usize::from(area.width);
    let body_height = usize::from(area.height).max(1);
    page.body_height = body_height;
    let document = timeline_document(width, page, theme);
    let total_rows = document.header.len().saturating_add(document.row_count);
    let maximum = total_rows.saturating_sub(body_height);
    let offset = page.offset().min(maximum);
    page.set_offset(offset);
    let rows = (offset..offset.saturating_add(body_height).min(total_rows)).map(|index| {
        if index < document.header.len() {
            document.header[index].clone()
        } else {
            timeline_row(index - document.header.len(), width, &document, theme)
        }
    });
    render_rows(frame, area, rows, width);
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    page: &mut HistoryPage,
    key_mapping: &KeyMapping,
    theme: &Theme,
) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Reset)),
        area,
    );
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = usize::from(area.width);
    if page.view == HistoryView::Timeline
        && matches!(
            &page.state,
            HistoryLoadState::Ready | HistoryLoadState::Empty
        )
    {
        render_timeline(frame, area, page, theme);
        return;
    }
    let mut rows = match &page.state {
        HistoryLoadState::Loading => {
            let mut row = Vec::new();
            put(
                &mut row,
                2,
                "Loading execution history…",
                theme.history.metadata,
            );
            vec![row]
        }
        HistoryLoadState::Empty => {
            let mut row = Vec::new();
            put(
                &mut row,
                2,
                "Top 50 · no measured completed operations for this session.",
                theme.history.metadata,
            );
            vec![row]
        }
        HistoryLoadState::Error(error) => {
            let mut row = Vec::new();
            put(
                &mut row,
                2,
                format!("Execution history unavailable: {error}"),
                theme.log.error,
            );
            vec![row]
        }
        HistoryLoadState::Ready => ready_document(width, page, theme),
    };
    for warning in &page.warnings {
        let mut row = Vec::new();
        put(&mut row, 2, format!("! {warning}"), theme.log.warning);
        rows.push(row);
    }
    if rows.is_empty() {
        rows.push(blank());
    }
    let body_height = usize::from(area.height.saturating_sub(3)).max(1);
    page.body_height = body_height;
    let maximum = rows.len().saturating_sub(body_height);
    let offset = page.offset().min(maximum);
    page.set_offset(offset);
    let buffer = frame.buffer_mut();
    for (local_y, row) in rows.iter().skip(offset).take(body_height).enumerate() {
        let y = area.y + local_y as u16;
        for run in row {
            if run.x >= width {
                continue;
            }
            buffer.set_stringn(
                area.x + run.x as u16,
                y,
                &run.text,
                width - run.x,
                run.style,
            );
        }
    }
    let rule_y = area.bottom().saturating_sub(3);
    buffer.set_stringn(
        area.x + 2,
        rule_y,
        "─".repeat(width.saturating_sub(4)),
        width.saturating_sub(4),
        theme.history.separator.style(),
    );
    let footer = format!(
        "{}/{} row · {}/{} half · {}/{} page · {} back",
        key_mapping.label(Scope::History, Action::MoveDown),
        key_mapping.label(Scope::History, Action::MoveUp),
        key_mapping.label(Scope::History, Action::MoveDownHalf),
        key_mapping.label(Scope::History, Action::MoveUpHalf),
        key_mapping.label(Scope::History, Action::MoveDownFast),
        key_mapping.label(Scope::History, Action::MoveUpFast),
        key_mapping.label(Scope::History, Action::Exit),
    );
    let progress = format!(
        "{}-{}/{}",
        offset + 1,
        (offset + body_height).min(rows.len()),
        rows.len()
    );
    let footer_y = area.bottom().saturating_sub(2);
    buffer.set_stringn(
        area.x + 2,
        footer_y,
        clip_width(&footer, width.saturating_sub(progress.width() + 7)),
        width,
        theme.history.hint.style(),
    );
    if progress.width() + 2 <= width {
        buffer.set_stringn(
            area.right().saturating_sub(progress.width() as u16 + 2),
            footer_y,
            &progress,
            progress.width(),
            theme.history.progress.style(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_summaries_keep_styles_inside_wide_and_narrow_columns() {
        use crate::execution_history::{OperationFinish, OperationStart};
        use ratatui::style::Modifier;

        let source = "cargo --flag=one &&\necho '中文e\u{0301}👩‍💻' >> long/file/path";
        let call = ExecutionCall {
            sequence: 1,
            run_id: "run".into(),
            start_unix_ms: 0,
            end_unix_ms: Some(1),
            operation: OperationStart {
                call_id: "call".into(),
                turn_id: None,
                parent_id: None,
                kind: OperationKind::Command,
                name: "bash".into(),
                summary: OperationSummary::Command {
                    command: source.into(),
                },
            },
            finish: Some(OperationFinish {
                call_id: "call".into(),
                outcome: ExecutionOutcome::Success,
                duration: None,
                output_lines: None,
            }),
        };
        let original = call.clone();
        for width in [100, 60, 28, 12] {
            let rows = record_rows(width, &[&call], &Theme::ferra());
            let (start, columns) = if width >= 80 {
                (28, width - 64)
            } else {
                (10, width - 12)
            };
            let summary_runs = rows[2]
                .iter()
                .filter(|run| run.x >= start && run.x < start + columns)
                .collect::<Vec<_>>();
            let text = summary_runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>();
            assert_eq!(
                text,
                crate::wrap::ellipsize_text(&source.replace('\n', " "), columns)
            );
            assert!(summary_runs
                .iter()
                .all(|run| run.x + run.text.width() <= start + columns));
            if width >= 60 {
                assert!(summary_runs.iter().any(|run| run.text == "--flag="
                    && run.style.add_modifier.contains(Modifier::ITALIC)));
                assert!(summary_runs
                    .iter()
                    .filter(|run| run.text.contains("one"))
                    .all(|run| !run.style.add_modifier.contains(Modifier::ITALIC)));
            }
            assert!(rows
                .iter()
                .flatten()
                .filter(|run| run.text == "✓")
                .all(|run| !run.style.add_modifier.contains(Modifier::ITALIC)));
        }
        assert_eq!(
            call, original,
            "presentation never decorates recorded source"
        );
    }

    #[test]
    fn clipping_and_clock_are_cell_bounded() {
        assert_eq!(clip_width("cargo", 3), "ca…");
        assert_eq!(clip_width("cargo", 1), "…");
        assert_eq!(clip_width("cargo", 0), "");
        assert_eq!(
            clock(14 * 3_600_000 + 32 * 60_000 + 39_123, true),
            "14:32:39.123"
        );
    }

    #[test]
    fn empty_ranking_displays_capture_diagnostics() {
        use ratatui::{backend::TestBackend, Terminal};

        let mut page = HistoryPage::loading(1, "session".into(), "root".into());
        page.state = HistoryLoadState::Empty;
        page.warnings.push("capture incomplete".into());
        let mut terminal = Terminal::new(TestBackend::new(100, 10)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &mut page,
                    &KeyMapping::default(),
                    &Theme::ferra(),
                )
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Top 50 · no measured completed operations"));
        assert!(text.contains("capture incomplete"));
    }

    #[test]
    fn page_resets_background_and_keeps_footer_fixed_while_document_scrolls() {
        use crate::execution_history::{
            ExecutionEvent, ExecutionRecord, MeasuredDuration, ObservedOutputLines,
            OperationFinish, OperationStart, TimingSource,
        };
        use ratatui::{backend::TestBackend, Terminal};

        let mut records = Vec::new();
        for index in 0..40_u64 {
            records.push(ExecutionRecord {
                sequence: index * 2 + 1,
                run_id: "run".into(),
                time_unix_ms: 50_000 + index * 100,
                event: ExecutionEvent::Started(OperationStart {
                    call_id: format!("call-{index}"),
                    turn_id: Some(format!("turn-{}", index / 3 + 1)),
                    parent_id: None,
                    kind: if index % 2 == 0 {
                        OperationKind::Command
                    } else {
                        OperationKind::Model
                    },
                    name: if index % 2 == 0 {
                        "bash".into()
                    } else {
                        "model".into()
                    },
                    summary: if index % 2 == 0 {
                        OperationSummary::Command {
                            command: "cargo test --workspace".into(),
                        }
                    } else {
                        OperationSummary::Identity
                    },
                }),
            });
            records.push(ExecutionRecord {
                sequence: index * 2 + 2,
                run_id: "run".into(),
                time_unix_ms: 50_050 + index * 100,
                event: ExecutionEvent::Finished(OperationFinish {
                    call_id: format!("call-{index}"),
                    outcome: ExecutionOutcome::Success,
                    duration: Some(MeasuredDuration {
                        duration_ms: 50 + index,
                        source: TimingSource::Backend,
                    }),
                    output_lines: Some(ObservedOutputLines {
                        count: index as usize + 1,
                        truncated: index == 7,
                    }),
                }),
            });
        }
        let calls = crate::execution_history::calls_from_records(&records);
        let ranked_calls = crate::execution_history::longest_calls(&calls, 50)
            .into_iter()
            .cloned()
            .collect();
        let mut page = HistoryPage::loading(1, "session".into(), "root".into());
        page.complete(
            1,
            crate::execution_history::HistoryQueryResult {
                path: "trace".into(),
                records: Vec::new(),
                ranked_calls,
                warnings: Vec::new(),
                watermark: 1,
                next_offset: 1,
                has_more: false,
            },
        );
        let theme = Theme::ferra();
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &mut page,
                    &KeyMapping::default(),
                    &theme,
                )
            })
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 0)].bg, Color::Reset);
        let first = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(first.contains("Top 50"));
        assert!(first.contains("■ model"));
        assert!(!first.contains("turn-"));
        assert!(!first.contains("Tab"));
        assert!(first.find("89ms").unwrap() < first.find("88ms").unwrap());
        assert!(first.contains("Esc / q back"));
        page.set_offset(30);
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &mut page,
                    &KeyMapping::default(),
                    &theme,
                )
            })
            .unwrap();
        let scrolled = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!scrolled.contains("■ model"));
        assert!(scrolled.contains("Esc / q back"));
    }
}
