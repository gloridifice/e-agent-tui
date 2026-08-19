//! Transitional re-export of frontend copy selection.

pub use e_tui::copy::*;

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
            .rows_with(&state.transcript_cache, state.transcript.len(), || {
                crate::ui::copy_layout_rows(&state)
            })
            .is_empty());
        assert!(!cache
            .rows_with(&state.transcript_cache, state.transcript.len(), || {
                crate::ui::copy_layout_rows(&state)
            })
            .is_empty());
        assert_eq!(cache.rebuild_count(), 1);

        state.transcript_cache.generation += 1;
        assert!(!cache
            .rows_with(&state.transcript_cache, state.transcript.len(), || {
                crate::ui::copy_layout_rows(&state)
            })
            .is_empty());
        assert_eq!(cache.rebuild_count(), 2);
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
