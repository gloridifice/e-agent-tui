//! Copy mode (design §4.3, D10–D13): vim-style navigation and selection over
//! the assistant transcript. Copies ALWAYS extract the original markdown
//! source (via the unit→raw table); tables/code/mermaid are atomic blocks
//! selected whole; line selection crossing an atomic block upgrades to it.

use std::collections::{BTreeSet, HashMap};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{model::AppState, transcript_layout::CopyLayoutRow};

/// One navigable row: a rendered assistant line plus its provenance.
#[derive(Debug, Clone)]
pub struct CopyRow {
    pub unit: u64,
    pub raw_line: Option<usize>,
    pub atomic: bool,
    /// Rendered plain text WITHOUT the ui-layer assistant bar prefix.
    pub text: String,
    /// Index of this row in the whole-transcript line list (for scrolling).
    pub global_row: usize,
}

pub struct CopyMode {
    pub cursor: usize,
    /// Line-selection anchor (V).
    pub anchor: Option<usize>,
    /// Block-selection anchor: (row, col in chars).
    pub block: Option<(usize, usize)>,
    pub col: usize,
}

impl Default for CopyMode {
    fn default() -> Self {
        Self {
            cursor: 0,
            anchor: None,
            block: None,
            col: 0,
        }
    }
}

/// What the main loop should do after a copy-mode key.
pub enum CopyAction {
    None,
    /// Copy this text to the clipboard and leave copy mode.
    Copy(String),
    Exit,
    /// Toggle the collapsed window of this unit (Enter).
    ToggleExpand(u64),
    /// Move cursor; the payload is the new global row to keep visible.
    Moved(usize),
}

fn build_rows(layout: Vec<CopyLayoutRow>) -> Vec<CopyRow> {
    layout
        .into_iter()
        .map(|row| CopyRow {
            unit: row.unit,
            raw_line: row.raw_line,
            atomic: row.atomic,
            text: row.text,
            global_row: row.global_row,
        })
        .collect()
}

/// Width/generation-indexed copy provenance shared by key handling and the
/// following overlay frame. Invalid or tail-dirty transcript state is rebuilt
/// eagerly because its generation has not advanced yet.
#[derive(Default)]
pub struct CopyRowsCache {
    generation: u64,
    width: usize,
    message_count: usize,
    initialized: bool,
    rows: Vec<CopyRow>,
    #[cfg(test)]
    rebuilds: usize,
}

impl CopyRowsCache {
    pub fn rows_with<'a>(
        &'a mut self,
        state: &AppState,
        build_layout: impl FnOnce() -> Vec<CopyLayoutRow>,
    ) -> &'a [CopyRow] {
        let transcript = &state.transcript_cache;
        let reusable = self.initialized
            && transcript.valid
            && !transcript.tail_dirty
            && self.generation == transcript.generation
            && self.width == transcript.width
            && self.message_count == state.transcript.len();
        if !reusable {
            self.rows = build_rows(build_layout());
            #[cfg(test)]
            {
                self.rebuilds += 1;
            }
            self.generation = transcript.generation;
            self.width = transcript.width;
            self.message_count = state.transcript.len();
            self.initialized = true;
        }
        &self.rows
    }

    pub fn invalidate(&mut self) {
        self.initialized = false;
    }
}

impl CopyMode {
    pub fn handle_key(&mut self, key: &KeyEvent, rows: &[CopyRow], state: &AppState) -> CopyAction {
        if rows.is_empty() {
            return CopyAction::Exit;
        }
        let max = rows.len().saturating_sub(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if ctrl && key.code == KeyCode::Char('c') {
            return CopyAction::Exit;
        }
        if ctrl && key.code == KeyCode::Char('v') {
            self.block = Some((self.cursor, self.col));
            self.anchor = None;
            return CopyAction::None;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return CopyAction::Exit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor = (self.cursor + 1).min(max);
                return CopyAction::Moved(rows[self.cursor].global_row);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                return CopyAction::Moved(rows[self.cursor].global_row);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.col = self.col.saturating_sub(1);
                return CopyAction::None;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.col += 1;
                return CopyAction::None;
            }
            KeyCode::Char('0') => {
                self.col = 0;
                return CopyAction::None;
            }
            KeyCode::Char('$') => {
                self.col = rows[self.cursor].text.chars().count().saturating_sub(1);
                return CopyAction::None;
            }
            KeyCode::Char('g') => {
                self.cursor = 0;
                return CopyAction::Moved(rows[0].global_row);
            }
            KeyCode::Char('G') => {
                self.cursor = max;
                return CopyAction::Moved(rows[max].global_row);
            }
            KeyCode::Char('V') => {
                if self.anchor.is_some() {
                    self.anchor = None;
                } else {
                    self.anchor = Some(self.cursor);
                }
                self.block = None;
                return CopyAction::None;
            }
            KeyCode::Char('y') => {
                let text = selection_text(self, rows, &state.units);
                return match text {
                    Some(text) => CopyAction::Copy(text),
                    None => CopyAction::None,
                };
            }
            KeyCode::Enter => {
                let unit = rows[self.cursor].unit;
                return CopyAction::ToggleExpand(unit);
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                // Handled by the main loop through Moved.
                let step = 12usize;
                if key.code == KeyCode::PageUp {
                    self.cursor = self.cursor.saturating_sub(step);
                } else {
                    self.cursor = (self.cursor + step).min(max);
                }
                return CopyAction::Moved(rows[self.cursor].global_row);
            }
            _ => CopyAction::None,
        }
    }

    /// Selected row range for highlighting: (lo, hi) inclusive in copy rows.
    pub fn selection_range(&self, rows: &[CopyRow]) -> Option<(usize, usize)> {
        if rows.is_empty() {
            return None;
        }
        let (lo, hi) = if let Some((ar, _)) = self.block {
            (ar.min(self.cursor), ar.max(self.cursor))
        } else if let Some(anchor) = self.anchor {
            (anchor.min(self.cursor), anchor.max(self.cursor))
        } else {
            return None;
        };
        // Expand across atomic blocks (D13).
        let mut lo = lo;
        let mut hi = hi;
        let mut i = lo;
        while i <= hi {
            if rows[i].atomic {
                let seg = unit_segment(rows, rows[i].unit);
                lo = lo.min(seg.0);
                hi = hi.max(seg.1);
                i = seg.1 + 1;
            } else {
                i += 1;
            }
        }
        Some((lo, hi))
    }
}

/// [start, end] row span of one unit in the flattened list.
fn unit_segment(rows: &[CopyRow], unit: u64) -> (usize, usize) {
    let start = rows.iter().position(|r| r.unit == unit).unwrap_or(0);
    let mut end = start;
    while end + 1 < rows.len() && rows[end + 1].unit == unit {
        end += 1;
    }
    (start, end)
}

/// Build the copied text for the current selection — always raw markdown.
pub fn selection_text(
    cm: &CopyMode,
    rows: &[CopyRow],
    units: &HashMap<u64, String>,
) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    // Block selection: rectangle of rendered text; atomic rows upgrade.
    if let Some((ar, ac)) = cm.block {
        let (r_lo, r_hi) = (ar.min(cm.cursor), ar.max(cm.cursor));
        if rows[r_lo..=r_hi].iter().any(|r| r.atomic) {
            return line_selection_text(ar, cm.cursor, rows, units);
        }
        let (c_lo, c_hi) = (ac.min(cm.col), ac.max(cm.col));
        let mut out = String::new();
        for row in &rows[r_lo..=r_hi] {
            let chars: Vec<char> = row.text.chars().collect();
            let lo = c_lo.min(chars.len());
            let hi = (c_hi + 1).min(chars.len());
            if lo < hi {
                out.push_str(&chars[lo..hi].iter().collect::<String>());
            }
            out.push('\n');
        }
        return Some(out.trim_end().to_string());
    }
    let anchor = cm.anchor.unwrap_or(cm.cursor);
    line_selection_text(anchor, cm.cursor, rows, units)
}

fn line_selection_text(
    a: usize,
    b: usize,
    rows: &[CopyRow],
    units: &HashMap<u64, String>,
) -> Option<String> {
    let mut lo = a.min(b);
    let mut hi = a.max(b);
    // Expand across atomic blocks (D13).
    let mut i = lo;
    while i <= hi {
        if rows[i].atomic {
            let seg = unit_segment(rows, rows[i].unit);
            lo = lo.min(seg.0);
            hi = hi.max(seg.1);
            i = seg.1 + 1;
        } else {
            i += 1;
        }
    }

    let mut out: Vec<String> = Vec::new();
    let mut emitted_units: BTreeSet<u64> = BTreeSet::new();
    let mut emitted_lines: BTreeSet<(u64, usize)> = BTreeSet::new();
    for row in &rows[lo..=hi] {
        if row.atomic {
            // Whole-block raw source, emitted once (D11).
            if emitted_units.insert(row.unit) {
                if let Some(raw) = units.get(&row.unit) {
                    out.push(raw.trim_end().to_string());
                }
            }
        } else if let Some(rl) = row.raw_line {
            if emitted_lines.insert((row.unit, rl)) {
                if let Some(raw) = units.get(&row.unit) {
                    if let Some(line) = raw.lines().nth(rl) {
                        out.push(line.to_string());
                    }
                }
            }
        } else {
            // Fallback: rendered text.
            out.push(row.text.clone());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Theme;
    use crate::model::{AppState, Msg};
    use crate::render::{render_markdown, RenderOptions};

    fn flatten(state: &AppState) -> Vec<CopyRow> {
        build_rows(crate::ui::copy_layout_rows(state))
    }

    fn state_with(md: &str) -> AppState {
        let mut state = AppState::default();
        let lines = render_markdown(
            md,
            &Theme::ferra(),
            &mut 0,
            &RenderOptions::default(),
            &mut state.units,
        );
        state.msgs.push(Msg::Assistant {
            text: md.to_string(),
            lines,
            unit_start: 0,
        });
        state
    }

    #[test]
    fn single_line_copy_uses_raw() {
        let md = "# 标题\n\n正文第一行\n正文第二行";
        let state = state_with(md);
        let rows = flatten(&state);
        // Heading + margin rows precede the paragraph; locate by content.
        let idx = rows
            .iter()
            .position(|r| r.text == "正文第一行")
            .expect("paragraph row present");
        let mut cm = CopyMode::default();
        cm.cursor = idx;
        let text = selection_text(&cm, &rows, &state.units).unwrap();
        assert_eq!(text, "正文第一行");
    }

    #[test]
    fn atomic_block_copies_raw_source() {
        let md = "前文\n\n```rust\nfn main() {}\n```";
        let state = state_with(md);
        let rows = flatten(&state);
        let code_row = rows.iter().position(|r| r.atomic).unwrap();
        let mut cm = CopyMode::default();
        cm.cursor = code_row;
        let text = selection_text(&cm, &rows, &state.units).unwrap();
        assert_eq!(text, "```rust\nfn main() {}\n```");
    }

    /// Wrapped user blocks count by their WRAPPED rows in copy-mode global
    /// row math (the renderer pre-wraps at the content width).
    #[test]
    fn flatten_counts_wrapped_user_rows() {
        use crate::model::Msg;
        use crate::render::RenderLine;

        let mut state = AppState::default();
        state.transcript_cache.width = 12;
        state.msgs.push(Msg::User {
            text: "x".repeat(30),
        });
        state.msgs.push(Msg::Assistant {
            text: "t".into(),
            lines: vec![RenderLine {
                line: ratatui::text::Line::from("x"),
                unit: 0,
                raw_line: Some(0),
                atomic: false,
                fill: false,
            }],
            unit_start: 0,
        });
        let rows = flatten(&state);
        // User block = 2 padding rows + 3 wrapped rows; the assistant row
        // follows at global row 6 (gap row 5 in between).
        assert_eq!(rows[0].global_row, 6, "wrapped user rows counted");
    }

    #[test]
    fn copy_rows_cache_reuses_matching_generation_and_width() {
        let mut state = state_with("abcdef");
        state.transcript_cache.valid = true;
        state.transcript_cache.width = 3;
        state.transcript_cache.generation = 1;
        let mut cache = CopyRowsCache::default();
        assert!(!cache
            .rows_with(&state, || crate::ui::copy_layout_rows(&state))
            .is_empty());
        assert!(!cache
            .rows_with(&state, || crate::ui::copy_layout_rows(&state))
            .is_empty());
        assert_eq!(cache.rebuilds, 1);

        state.transcript_cache.generation += 1;
        assert!(!cache
            .rows_with(&state, || crate::ui::copy_layout_rows(&state))
            .is_empty());
        assert_eq!(cache.rebuilds, 2);
    }

    #[test]
    fn wrapped_source_line_is_copied_once() {
        let raw = "abcdefghijklmnopqrstuvwxyz";
        let mut state = state_with(raw);
        state.transcript_cache.width = 8;
        let rows = flatten(&state);
        assert!(rows.len() >= 4, "source line wraps into copy rows");
        let mut cm = CopyMode::default();
        cm.anchor = Some(0);
        cm.cursor = rows.len() - 1;
        assert_eq!(selection_text(&cm, &rows, &state.units).unwrap(), raw);
    }

    #[test]
    fn table_is_atomic_and_raw() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |";
        let state = state_with(md);
        let rows = flatten(&state);
        assert!(rows.iter().all(|r| r.atomic));
        let mut cm = CopyMode::default();
        cm.cursor = 2; // middle of table
        let text = selection_text(&cm, &rows, &state.units).unwrap();
        assert_eq!(text, "| a | b |\n|---|---|\n| 1 | 2 |");
    }

    #[test]
    fn line_selection_crossing_atomic_upgrades() {
        let md = "第一段\n\n```\ncode line\n```\n\n第二段";
        let state = state_with(md);
        let rows = flatten(&state);
        let mut cm = CopyMode::default();
        cm.anchor = Some(0);
        cm.cursor = rows.len() - 1;
        let text = selection_text(&cm, &rows, &state.units).unwrap();
        assert!(text.contains("```"), "atomic raw included: {text}");
        assert!(text.contains("code line"));
        assert!(text.contains("第一段"));
        assert!(text.contains("第二段"));
    }

    #[test]
    fn block_selection_rectangle() {
        let md = "abcdef\nghijkl";
        let state = state_with(md);
        let rows = flatten(&state);
        let mut cm = CopyMode::default();
        cm.block = Some((0, 1));
        cm.cursor = 1;
        cm.col = 3;
        let text = selection_text(&cm, &rows, &state.units).unwrap();
        assert_eq!(text, "bcd\nhij");
    }

    #[test]
    fn flatten_glues_activity_rows() {
        use crate::model::{FileAction, FileGroup, FileItem, ToolCard, ToolState};
        use crate::render::RenderLine;
        use ratatui::text::Line;

        let mut state = AppState::default();
        state.msgs.push(Msg::Tool(ToolCard {
            call_id: "b".into(),
            name: "bash".into(),
            summary: "s".into(),
            state: ToolState::Running,
            frame: 0,
            start_ms: 0,
            done_since: None,
            done_from: None,
        }));
        state.msgs.push(Msg::FileGroup(FileGroup {
            items: vec![FileItem {
                action: FileAction::Edit,
                call_id: "e".into(),
                file: "a.rs".into(),
                ok: None,
            }],
            frame: 0,
            done_since: None,
            done_from: None,
        }));
        state.msgs.push(Msg::Assistant {
            text: "t".into(),
            lines: vec![RenderLine {
                line: Line::from("x"),
                unit: 0,
                raw_line: Some(0),
                atomic: false,
                fill: false,
            }],
            unit_start: 0,
        });
        let rows = flatten(&state);
        assert_eq!(rows.len(), 1);
        // tool(1) glued group(1) → no gap; gap before the assistant row.
        assert_eq!(rows[0].global_row, 3);
    }
}
