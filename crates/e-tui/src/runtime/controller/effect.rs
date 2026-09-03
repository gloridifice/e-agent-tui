//! Effect completion, config reload, and deferred queue controller behavior.

use super::{
    agent_action, paste_text, AgentRequest, ClipboardPaste, Config, EffectResult, InputState,
    Instant, Mutex, RuntimeState, TerminalUiState, ThemeFile, UiAction,
};
use crate::i18n::{tr, tr_args};

pub(super) fn apply_reloaded_config(
    config: Config,
    themes: Vec<ThemeFile>,
    state: &Mutex<RuntimeState>,
    ui: &mut TerminalUiState<'_>,
) {
    *ui.config = config;
    *ui.themes = themes;
    *ui.theme = ui.config.theme();
    ui.input.language = ui.config.language;
    ui.input.paste_placeholder_chars = ui.config.paste_placeholder_chars;
    ui.input.history_limit = ui.config.history_limit;
    let catalogs = {
        let mut state = state.lock().unwrap();
        state.config = ui.config.clone();
        state.render.markdown_layout.invalidate_all();
        state.render.transcript_cache.invalidate();
        state.preview.invalidate_layout();
        let language = state.config.language;
        state.push_system_message(tr(language, "controller.reload"));
        state.catalogs.clone()
    };
    ui.input.catalog_changed(&catalogs);
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
        EffectResult::ConfigPersisted(Err(error)) | EffectResult::ConfigReloadFailed(error) => {
            let mut app = state.lock().unwrap();
            let language = app.config.language;
            app.push_error_message(tr_args(
                language,
                "controller.config_save_failed",
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
    let Some(prompt) = state.take_next_queued() else {
        return Vec::new();
    };
    state.start_thinking();
    vec![agent_action(AgentRequest::Input { prompt })]
}
