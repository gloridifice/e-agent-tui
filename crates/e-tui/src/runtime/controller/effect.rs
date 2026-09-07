//! Effect completion, config reload, and deferred queue controller behavior.

use super::{
    agent_action, paste_text, AgentRequest, ClipboardPaste, Config, EffectResult, InputState,
    Instant, Mutex, RuntimeState, TerminalUiState, Theme, ThemeFile, UiAction,
};
use crate::i18n::{tr, tr_args};

/// Apply the derived state shared by settings changes and config reloads.
pub(super) fn sync_live_config(
    config: &Config,
    state: &Mutex<RuntimeState>,
    input: &mut InputState,
    theme: &mut Theme,
) {
    *theme = config.theme();
    input.language = config.language;
    input.key_mapping = config.key_mapping.clone();
    input.paste_placeholder_chars = config.paste_placeholder_chars;
    input.history_limit = config.history_limit;
    let catalogs = {
        let mut state = state.lock().unwrap();
        state.config = config.clone();
        state.render.markdown_layout.invalidate_all();
        state.render.transcript_cache.invalidate();
        state.preview.invalidate_layout();
        state.catalogs.clone()
    };
    input.catalog_changed(&catalogs);
}

pub(super) fn apply_reloaded_config(
    config: Config,
    themes: Vec<ThemeFile>,
    state: &Mutex<RuntimeState>,
    ui: &mut TerminalUiState<'_>,
) {
    let mut config = config;
    if let Some(error) = config.key_mapping_error.take() {
        config.key_mapping = ui.config.key_mapping.clone();
        state.lock().unwrap().push_error_message(error);
    }
    *ui.config = config;
    *ui.themes = themes;
    sync_live_config(ui.config, state, ui.input, ui.theme);
    let mut state = state.lock().unwrap();
    let language = state.config.language;
    state.push_system_message(tr(language, "controller.reload"));
}

pub(super) fn apply_effect_result(
    result: EffectResult,
    state: &Mutex<RuntimeState>,
    now: Instant,
) -> bool {
    match result {
        EffectResult::ConfigPersisted(Ok(())) | EffectResult::ConfigReloaded { .. } => false,
        EffectResult::ClipboardRead(Ok(content)) => {
            let mut app = state.lock().unwrap();
            if app.reading.is_some()
                || app.interaction.approval.is_some()
                || app.interaction.help_visible
            {
                return false;
            }
            let interaction = &mut app.interaction;
            match content {
                ClipboardPaste::Text(text) => {
                    let text = crate::input::normalize_paste_text(&text);
                    paste_text(&mut interaction.input, &mut interaction.input_page, &text)
                }
                ClipboardPaste::Image(image) => {
                    if interaction.input_page.is_some() {
                        false
                    } else {
                        interaction.input.paste_image(image);
                        true
                    }
                }
            }
        }
        EffectResult::ClipboardRead(Err(error)) => {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.push_error_message(tr_args(
                language,
                "controller.clipboard_read_failed",
                &[("error", error)],
            ));
            true
        }
        EffectResult::ClipboardWritten {
            lines,
            preview,
            truncated,
        } => {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.interaction
                .notice
                .show_clipboard(language, lines, &preview, truncated, now);
            true
        }
        EffectResult::ConfigPersisted(Err(error)) => {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.push_error_message(tr_args(
                language,
                "controller.config_save_failed",
                &[("error", error)],
            ));
            true
        }
        EffectResult::ConfigReloadFailed(error) => {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.push_error_message(tr_args(
                language,
                "controller.config_reload_failed",
                &[("error", error)],
            ));
            true
        }
        EffectResult::ClipboardFailed(error) => {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.push_error_message(tr_args(
                language,
                "controller.clipboard_write_failed",
                &[("error", error)],
            ));
            if app.reading.is_some() {
                let placeholder = InputState::new(&app.config);
                let mut input = std::mem::replace(&mut app.interaction.input, placeholder);
                app.exit_reading(&mut input);
                app.interaction.input = input;
            }
            true
        }
        EffectResult::PreviewResolved {
            request_id,
            key,
            revision,
            result,
        } => state
            .lock()
            .unwrap()
            .preview
            .complete(request_id, key, revision, result),
    }
}

pub(super) fn dispatch_next_queued(state: &Mutex<RuntimeState>) -> Vec<UiAction> {
    let mut state = state.lock().unwrap();
    let was_idle = state.is_agent_idle();
    let Some(pending) = state.take_next_queued() else {
        return Vec::new();
    };
    let steering = pending.delivery == crate::interaction::PromptDelivery::Asap
        && (!was_idle || state.interaction.queue.has_backend());
    let request = if steering {
        state.interaction.queue.begin_submission(pending.clone());
        AgentRequest::Steer {
            prompt: pending.prompt,
        }
    } else {
        state.admit_submission(&pending.prompt, was_idle);
        AgentRequest::Input {
            prompt: pending.prompt,
        }
    };
    vec![agent_action(request)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{display::DisplayItem, Language};

    fn last_block_text(state: &Mutex<RuntimeState>) -> String {
        let state = state.lock().unwrap();
        let Some(DisplayItem::Block(block)) =
            state.transcript.nodes().last().map(|node| &node.item)
        else {
            panic!("expected an error block");
        };
        block.content.clone()
    }

    #[test]
    fn config_failures_use_operation_specific_messages() {
        let save_state = Mutex::new(RuntimeState::default());
        assert!(apply_effect_result(
            EffectResult::ConfigPersisted(Err("write failed".into())),
            &save_state,
            Instant::now(),
        ));
        assert_eq!(
            last_block_text(&save_state),
            "Settings save failed: write failed"
        );

        let reload_state = Mutex::new(RuntimeState::default());
        reload_state.lock().unwrap().config.language = Language::SimplifiedChinese;
        assert!(apply_effect_result(
            EffectResult::ConfigReloadFailed("read failed".into()),
            &reload_state,
            Instant::now(),
        ));
        assert_eq!(last_block_text(&reload_state), "配置重载失败：read failed");
    }
}
