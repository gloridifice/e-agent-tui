//! Central slash-command registry and dispatcher.
//!
//! Every optimized (built-in) command is declared exactly once in
//! [`BUILTIN_COMMANDS`]: the same entry owns its description, argument
//! completion policy, and runtime effect. DSH/plugin commands arrive as
//! [`CommandInfo`] descriptors and are merged into discovery automatically;
//! built-ins win name collisions while unknown names use the generic bridge
//! executor.

use std::sync::{Arc, Mutex};

pub use crate::command_catalog::{
    builtin_command, completion_context, match_command_catalog, BuiltinCommand, CommandCandidate,
    CommandSource, CompletionKind, BUILTIN_COMMANDS,
};
#[cfg(test)]
use crate::model::Msg;
use crate::{
    command_catalog::{CommandAction, NewMode},
    config::{Config, Theme},
    copy::CopyMode,
    input_page::InputPageSession,
    model::AppState,
    protocol::ClientMessage,
    settings,
    theme::ThemeFile,
};

#[derive(Default)]
pub struct CommandOutcome {
    pub outbound: Vec<ClientMessage>,
    pub reload_config: bool,
    pub quit: bool,
}

pub struct LocalCommandContext<'a> {
    pub input_page: &'a mut Option<InputPageSession>,
    pub help_visible: &'a mut bool,
    pub copy_mode: &'a mut Option<CopyMode>,
    pub config: &'a mut Config,
    pub themes: &'a mut Vec<ThemeFile>,
    pub new_modes: &'a [NewMode],
    pub input_paste_placeholder_chars: &'a mut usize,
    pub input_history_limit: &'a mut usize,
    pub theme: &'a mut Theme,
    pub state: &'a Arc<Mutex<AppState>>,
}

fn parse_line(line: &str) -> Option<(&str, &str)> {
    let body = line.strip_prefix('/')?;
    let split = body.find(char::is_whitespace).unwrap_or(body.len());
    let (name, raw_input) = body.split_at(split);
    (!name.is_empty()).then_some((name, raw_input))
}

fn reject_arguments(context: &LocalCommandContext<'_>, command: &str, raw_input: &str) -> bool {
    if raw_input.trim().is_empty() {
        return false;
    }
    push_error(context.state, format!("用法: /{command}"));
    true
}

/// Append a client-side usage error through the public transcript surface.
fn push_error(state: &Arc<Mutex<AppState>>, text: impl Into<String>) {
    state.lock().unwrap().push_error_message(text);
}

fn forward(line: String, outcome: &mut CommandOutcome) {
    outcome.outbound.push(ClientMessage::Command { line });
}

fn new_command_line(raw_input: &str, default_mode: &str) -> Option<String> {
    let mode = raw_input.trim();
    if mode.is_empty() {
        let default_mode = match default_mode.trim() {
            "" => "standard",
            configured => configured,
        };
        return Some(format!("/new {default_mode}"));
    }
    (mode.split_whitespace().count() == 1).then(|| format!("/new {mode}"))
}

pub fn handle_local_command(line: String, context: LocalCommandContext<'_>) -> CommandOutcome {
    let mut outcome = CommandOutcome::default();
    let Some((name, raw_input)) = parse_line(&line) else {
        return outcome;
    };
    let Some(command) = builtin_command(name) else {
        // Auto-discovered DSH/plugin command: generic command-plane adapter.
        forward(line, &mut outcome);
        return outcome;
    };

    match command.action {
        CommandAction::Settings => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            *context.input_page = Some(InputPageSession::settings(settings::SettingsState {
                modes: context
                    .new_modes
                    .iter()
                    .map(|mode| mode.id.clone())
                    .collect(),
                themes: context
                    .themes
                    .iter()
                    .map(|theme| theme.name.clone())
                    .collect(),
                ..Default::default()
            }));
        }
        CommandAction::Login => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            *context.input_page = Some(InputPageSession::login());
            outcome.outbound.push(ClientMessage::LoginGet);
        }
        CommandAction::Theme => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            *context.input_page = Some(InputPageSession::theme(
                context.themes,
                &context.config.theme,
            ));
        }
        CommandAction::Model => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            *context.input_page = Some(InputPageSession::model());
            outcome.outbound.push(ClientMessage::ModelGet);
        }
        CommandAction::Reload => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            outcome.reload_config = true;
        }
        CommandAction::Help => {
            if !reject_arguments(&context, name, raw_input) {
                *context.help_visible = true;
            }
        }
        CommandAction::Copy => {
            if !reject_arguments(&context, name, raw_input) {
                *context.copy_mode = Some(CopyMode::default());
            }
        }
        CommandAction::Quit => {
            if !reject_arguments(&context, name, raw_input) {
                outcome.quit = true;
                return outcome;
            }
        }
        CommandAction::Resume => {
            let session_id = raw_input.trim();
            if session_id.is_empty() {
                *context.input_page = Some(InputPageSession::resume());
                outcome.outbound.push(ClientMessage::ListSessions);
            } else if session_id.split_whitespace().count() == 1 {
                outcome.outbound.push(ClientMessage::Attach {
                    session_id: session_id.to_owned(),
                });
            } else {
                push_error(context.state, "用法: /resume [会话 ID]");
            }
        }
        CommandAction::New => {
            if let Some(line) = new_command_line(raw_input, &context.config.default_mode) {
                forward(line, &mut outcome);
            } else {
                push_error(context.state, "用法: /new [模式]");
            }
        }
        // Bridge-optimized commands and known DSH commands still use the
        // command plane; their declaration remains local so richer argument
        // completion can be added without duplicating metadata elsewhere.
        CommandAction::Skill | CommandAction::Forward => forward(line, &mut outcome),
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{CommandInfo, CommandInputInfo};

    #[test]
    fn catalog_merges_integrated_commands_and_builtins_shadow_duplicates() {
        let integrated = vec![
            CommandInfo {
                name: "feedback".into(),
                description: "record feedback".into(),
                input: Some(CommandInputInfo {
                    hint: "<text>".into(),
                }),
            },
            CommandInfo {
                name: "plan".into(),
                description: "host plan".into(),
                input: None,
            },
        ];
        let all = match_command_catalog("", &integrated);
        assert_eq!(all.iter().filter(|item| item.line == "/plan").count(), 1);
        let feedback = all.iter().find(|item| item.line == "/feedback").unwrap();
        assert_eq!(feedback.source, CommandSource::Integrated);
        assert!(feedback.description.contains("<text>"));
    }

    #[test]
    fn optimized_command_owns_completion_and_action_in_one_entry() {
        let new = builtin_command("new").unwrap();
        assert_eq!(new.completion, CompletionKind::NewMode);
        assert_eq!(new.action, CommandAction::New);
        assert_eq!(completion_context("/new m").unwrap().1, "m");
        assert_eq!(completion_context("/skill").unwrap().1, "");
        assert_eq!(completion_context("/skill:code").unwrap().1, "code");
        assert_eq!(completion_context("/skill code").unwrap().1, "code");
        assert!(completion_context("/resume ").is_none());
    }

    #[test]
    fn bare_new_uses_the_configured_default_mode() {
        assert_eq!(new_command_line("", "cordis"), Some("/new cordis".into()));
        assert_eq!(
            new_command_line(" minimal ", "cordis"),
            Some("/new minimal".into())
        );
        assert_eq!(new_command_line("minimal extra", "cordis"), None);
        assert_eq!(new_command_line("", "  "), Some("/new standard".into()));
    }

    #[test]
    fn push_error_appends_and_invalidates_the_cache() {
        let state = Arc::new(Mutex::new(AppState::default()));
        state.lock().unwrap().transcript_cache.valid = true;
        push_error(&state, "用法: /settings");
        let state = state.lock().unwrap();
        assert!(matches!(
            state.msgs.last(),
            Some(Msg::Error { text }) if text == "用法: /settings"
        ));
        assert!(
            !state.transcript_cache.valid,
            "error must invalidate the cache"
        );
    }

    #[test]
    fn fuzzy_ranking_is_prefix_then_substring_then_subsequence() {
        assert_eq!(match_command_catalog("set", &[])[0].line, "/settings");
        assert_eq!(
            match_command_catalog("ett", &[])
                .into_iter()
                .map(|item| item.line)
                .collect::<Vec<_>>(),
            vec!["/settings"]
        );
        assert_eq!(
            match_command_catalog("pln", &[])
                .into_iter()
                .map(|item| item.line)
                .collect::<Vec<_>>(),
            vec!["/plan"]
        );
    }
}
