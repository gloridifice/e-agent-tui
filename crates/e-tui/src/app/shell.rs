use super::*;
use crate::{
    display::{ActivityRow, ActivityState, ContentCard, DisplayId, DisplayTone},
    i18n::{tr, tr_args},
    shell::{ShellRequest, ShellResult},
};

impl TuiApp {
    pub fn start_shell(&mut self, command: String) -> ShellRequest {
        let id = DisplayId::correlated("local-shell", &self.shell.next_id.to_string());
        self.shell.next_id = self.shell.next_id.wrapping_add(1);
        self.shell.active = Some(id.clone());
        if self.session.new_conversation.is_some() {
            self.shell.draft_ids.insert(id.clone());
        }
        let mut activity = ActivityRow::tool(id.clone(), format!("!{command}"));
        activity.summary = tr(self.config.language, "shell.running");
        activity.live_duration_since = Some(Instant::now());
        activity.output_lines = Some(0);
        let unit = self.render.next_unit;
        self.render.next_unit += 1;
        self.render.units.insert(unit, String::new());
        let detail = ContentCard {
            id: id.clone(),
            unit: Some(unit),
            header: None,
            content: String::new(),
            role: CardRole::Terminal,
            tone: DisplayTone::Normal,
            horizontal_padding: 2,
            copy_source: String::new(),
        };
        self.timeline
            .transcript
            .append(DisplayItem::Composite { activity, detail }, None);
        self.render.transcript_cache.invalidate();
        self.reconcile_latest_preview();
        ShellRequest {
            id,
            command,
            cwd: self.session.session_cwd.clone(),
        }
    }

    pub fn finish_shell(&mut self, id: DisplayId, result: ShellResult) -> bool {
        if self.shell.active.as_ref() != Some(&id) {
            return false;
        }
        self.shell.active = None;
        let Some(node) = self.timeline.transcript.get(&id) else {
            return false;
        };
        let DisplayItem::Composite { activity, detail } = &node.item else {
            return false;
        };
        let mut activity = activity.clone();
        let mut detail = detail.clone();
        activity.state = if result.cancelled {
            ActivityState::Cancelled
        } else if result.error.is_some() || result.exit_code != Some(0) {
            ActivityState::Failure
        } else {
            ActivityState::Success
        };
        activity.summary = if result.cancelled {
            tr(self.config.language, "shell.cancelled")
        } else if let Some(code) = result.exit_code {
            format!("exit {code}")
        } else {
            tr(self.config.language, "shell.failed")
        };
        activity.duration_ms = Some(result.duration_ms);
        activity.live_duration_since = None;
        detail.content = result.output;
        if let Some(error) = result.error {
            if !detail.content.is_empty() && !detail.content.ends_with('\n') {
                detail.content.push('\n');
            }
            detail.content.push_str(&tr_args(
                self.config.language,
                "shell.error",
                &[("error", error)],
            ));
        }
        activity.output_lines_truncated =
            crate::shell::bound_output(&mut detail.content) || result.output_truncated;
        activity.output_lines = Some(detail.content.lines().count());
        detail.copy_source = detail.content.clone();
        if let Some(unit) = detail.unit {
            self.render.units.insert(unit, detail.copy_source.clone());
        }
        self.timeline
            .transcript
            .append(DisplayItem::Composite { activity, detail }, None);
        self.shell.unfolded.insert(id);
        self.render.transcript_cache.invalidate();
        self.rebuild_reading_model();
        if self.reading.is_some() {
            self.sync_reading_preview();
        } else {
            self.reconcile_latest_preview();
        }
        true
    }

    pub(crate) fn fold_shell_output(&mut self) {
        if !self.shell.unfolded.is_empty() {
            self.shell.unfolded.clear();
            self.render.transcript_cache.invalidate();
        }
    }

    pub(crate) fn clear_shell(&mut self) {
        if let Some(id) = self.shell.active.take() {
            self.pending_actions.push(UiAction::CancelShell(id));
        }
        self.shell.draft_ids.clear();
        self.shell.unfolded.clear();
    }

    pub fn reading_terminal_owner(&self) -> Option<&DisplayId> {
        let owner = &self
            .reading
            .as_ref()?
            .current(&self.reading_document)?
            .owner;
        matches!(&self.timeline.transcript.get(owner)?.item,
            DisplayItem::Composite { detail, .. } if detail.role == CardRole::Terminal
        )
        .then_some(owner)
    }

    pub(super) fn refresh_terminal_reading_geometry(
        &mut self,
        previous: Option<DisplayId>,
        scroll: &ScrollState,
        height: usize,
    ) {
        let current = self.reading_terminal_owner().cloned();
        if previous == current {
            return;
        }
        self.render.transcript_cache.invalidate();
        if let Some(reading) = &self.reading {
            if let Some(geometry) = self.reading_layout.block(&reading.block_cursor) {
                self.reading_layout_anchor = Some(ReadingLayoutAnchor {
                    block: reading.block_cursor.clone(),
                    screen_row: geometry.rows.start.saturating_sub(scroll.offset),
                    viewport_height: height,
                });
            }
        }
    }
}
