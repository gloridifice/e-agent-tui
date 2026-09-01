//! Central slash-command registry and dispatcher.
//!
//! Every optimized (built-in) command is declared exactly once in
//! [`BUILTIN_COMMANDS`]: the same entry owns its description, argument
//! completion policy, and runtime effect. DSH/plugin commands arrive as
//! [`CommandDescriptor`] descriptors and are merged into discovery automatically;
//! built-ins win name collisions while unknown names use the generic bridge
//! executor.

use std::sync::{Arc, Mutex};

pub use crate::command_catalog::{
    builtin_command, completion_context, match_command_catalog, BuiltinCommand, CommandCandidate,
    CommandSource, CompletionKind, BUILTIN_COMMANDS,
};
use crate::runtime::state::RuntimeState;
use crate::{
    command_catalog::{CommandAction, NewMode},
    input_page::InputPageSession,
    settings, ThemeFile,
};
use crate::{AgentRequest, Config, Theme};

#[derive(Default)]
pub struct CommandOutcome {
    pub outbound: Vec<AgentRequest>,
    /// The outbound command runs through `commands.execute` and remains
    /// interruptible until its direct result/error arrives.
    pub starts_interruptible_command: bool,
    pub reload_config: bool,
    pub new_conversation: bool,
    pub activate_reading: bool,
    pub quit: bool,
}

pub struct LocalCommandContext<'a> {
    pub input_page: &'a mut Option<InputPageSession>,
    pub help_visible: &'a mut bool,
    pub config: &'a mut Config,
    pub themes: &'a mut Vec<ThemeFile>,
    pub new_modes: &'a [NewMode],
    pub input_paste_placeholder_chars: &'a mut usize,
    pub input_history_limit: &'a mut usize,
    pub theme: &'a mut Theme,
    /// A question page or approval card is open (the caller reads the live
    /// interaction, which is not reachable through the state lock while the
    /// main loop holds the InteractionModel out of RuntimeState).
    pub question_open: bool,
    pub approval_open: bool,
    pub state: &'a Arc<Mutex<RuntimeState>>,
}

fn parse_line(line: &str) -> Option<(&str, &str)> {
    let body = line.strip_prefix('/')?;
    let split = body.find(char::is_whitespace).unwrap_or(body.len());
    let (name, raw_input) = body.split_at(split);
    (!name.is_empty()).then_some((name, raw_input))
}

fn is_colon_skill_invocation(name: &str) -> bool {
    name.split_once(':')
        .is_some_and(|(prefix, skill)| prefix.eq_ignore_ascii_case("skill") && !skill.is_empty())
}

fn reject_arguments(context: &LocalCommandContext<'_>, command: &str, raw_input: &str) -> bool {
    if raw_input.trim().is_empty() {
        return false;
    }
    push_error(context.state, format!("用法: /{command}"));
    true
}

/// Append a client-side usage error through the public transcript surface.
fn push_error(state: &Arc<Mutex<RuntimeState>>, text: impl Into<String>) {
    state.lock().unwrap().push_error_message(text);
}

fn has_new_conversation(state: &Arc<Mutex<RuntimeState>>) -> bool {
    state.lock().unwrap().is_new_conversation()
}

fn set_new_conversation_notice(state: &Arc<Mutex<RuntimeState>>, text: impl Into<String>) {
    state.lock().unwrap().set_new_conversation_notice(text);
}

fn forward(line: String, outcome: &mut CommandOutcome, interruptible: bool) {
    outcome.outbound.push(AgentRequest::Command {
        line,
        images: Vec::new(),
    });
    outcome.starts_interruptible_command = interruptible;
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
        // A client-only draft is not attached to an agent of its own. Never
        // let an integrated command mutate the retained old session.
        if has_new_conversation(context.state) {
            set_new_conversation_notice(context.state, "请先发送一条消息创建新对话");
        } else {
            let interruptible = !is_colon_skill_invocation(name);
            forward(line, &mut outcome, interruptible);
        }
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
            outcome.outbound.push(AgentRequest::LoginGet);
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
            // The model picker is session-independent: providers and models
            // come from the host catalog, and a pending selection is applied
            // to the next materialized session (including a deferred `/new`).
            *context.input_page = Some(InputPageSession::model());
            outcome.outbound.push(AgentRequest::ModelGet);
        }
        CommandAction::Effort => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            // The effort picker reads the exact current route's adapter-declared
            // efforts from the same model catalog; the selected effort is applied
            // to the materialized session (including a deferred `/new`).
            *context.input_page = Some(InputPageSession::effort());
            outcome.outbound.push(AgentRequest::ModelGet);
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
        CommandAction::Reading => {
            if !reject_arguments(&context, name, raw_input) {
                outcome.activate_reading = true;
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
                outcome.outbound.push(AgentRequest::ListSessions);
            } else if session_id.split_whitespace().count() == 1 {
                outcome.outbound.push(AgentRequest::Attach {
                    session_id: session_id.to_owned(),
                });
            } else {
                push_error(context.state, "用法: /resume [会话 ID]");
            }
        }
        CommandAction::New => {
            let blocked = context.question_open || context.approval_open;
            let materializing = context
                .state
                .lock()
                .unwrap()
                .session
                .new_conversation
                .as_ref()
                .is_some_and(|draft| draft.pending_input.is_some());
            if blocked {
                push_error(context.state, "请先完成当前提问或审批，再新建对话");
            } else if materializing {
                set_new_conversation_notice(context.state, "正在创建新对话，请稍候");
            } else if let Some(line) = new_command_line(raw_input, &context.config.default_mode) {
                let mode = line.trim_start_matches("/new ").to_owned();
                context.state.lock().unwrap().begin_new_conversation(mode);
                outcome.new_conversation = true;
            } else {
                push_error(context.state, "用法: /new [模式]");
            }
        }
        // Bridge-optimized commands and known DSH commands still use the
        // command plane; their declaration remains local so richer argument
        // completion can be added without duplicating metadata elsewhere.
        CommandAction::Skill | CommandAction::Forward => {
            if has_new_conversation(context.state) {
                set_new_conversation_notice(context.state, "请先发送一条消息创建新对话");
            } else {
                // `/skill` is injected as a model follow-up and is covered by
                // agent status. Other forwarded commands run directly through
                // DSH's abortable command executor.
                forward(line, &mut outcome, command.action == CommandAction::Forward);
            }
        }
    }
    outcome
}
