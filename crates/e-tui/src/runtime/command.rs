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
    CommandSource, CompletionKind, FixedSubcommandAction, BUILTIN_COMMANDS,
};
use crate::runtime::state::RuntimeState;
use crate::{
    agent::{CommandDescriptor, ModelProvider, ModelSelection},
    AgentRequest, Config, Theme,
};
use crate::{
    catalog::resolve_model_reference,
    command_catalog::{
        parse_command_line as parse_line, resolve_fixed_subcommand, CommandAction, NewMode,
    },
    i18n::{tr, tr_args, Language},
    input_page::InputPageSession,
    settings, ThemeFile,
};

#[derive(Default)]
pub struct CommandOutcome {
    pub outbound: Vec<AgentRequest>,
    /// The outbound command runs through `commands.execute` and remains
    /// interruptible until its direct result/error arrives.
    pub starts_interruptible_command: bool,
    pub reload_config: bool,
    pub config_changed: bool,
    pub new_conversation: bool,
    pub activate_reading: bool,
    pub history: Option<FixedSubcommandAction>,
    pub copy_markdown: Option<String>,
    pub open_help: bool,
    pub quit: bool,
}

pub struct LocalCommandContext<'a> {
    pub language: Language,
    pub input_page: &'a mut Option<InputPageSession>,
    pub integrated_commands: &'a [CommandDescriptor],
    pub config: &'a mut Config,
    pub themes: &'a mut Vec<ThemeFile>,
    pub new_modes: &'a [NewMode],
    pub model_providers: &'a [ModelProvider],
    pub current_model: Option<&'a ModelSelection>,
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

fn is_colon_skill_invocation(name: &str) -> bool {
    name.split_once(':')
        .is_some_and(|(prefix, skill)| prefix.eq_ignore_ascii_case("skill") && !skill.is_empty())
}

fn reject_arguments(context: &LocalCommandContext<'_>, command: &str, raw_input: &str) -> bool {
    if raw_input.trim().is_empty() {
        return false;
    }
    push_error(
        context.state,
        tr_args(
            context.language,
            "command.usage",
            &[("command", command.to_owned())],
        ),
    );
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

fn submit_skill(line: String, context: &LocalCommandContext<'_>, outcome: &mut CommandOutcome) {
    let prompt = crate::PromptInput::text(line.clone());
    let mut state = context.state.lock().unwrap();
    if state.is_new_conversation() {
        if let Some(request) = state.materialize_new_conversation(prompt) {
            outcome.outbound.push(request);
        } else {
            state.set_new_conversation_notice(tr(context.language, "command.new.in_progress"));
        }
    } else {
        state.admit_submission(&prompt, true);
        forward(line, outcome, false);
    }
}

fn resolve_effort_reference(
    providers: &[ModelProvider],
    current: Option<&ModelSelection>,
    reference: &str,
) -> Option<(String, String, String)> {
    let current = current?;
    let effort = providers
        .iter()
        .find(|provider| provider.id == current.provider)?
        .models
        .iter()
        .find(|model| model.id == current.model)?
        .reasoning
        .as_ref()?
        .efforts
        .iter()
        .find(|effort| effort.id.eq_ignore_ascii_case(reference.trim()))?;
    Some((
        current.provider.clone(),
        current.model.clone(),
        effort.id.clone(),
    ))
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
    if is_colon_skill_invocation(name) || (name == "skill" && !raw_input.trim().is_empty()) {
        submit_skill(line, &context, &mut outcome);
        return outcome;
    }
    let is_pi = context.state.lock().unwrap().frontend == crate::FrontendKind::Pi;
    if name == "logout" && is_pi {
        if !raw_input.trim().is_empty() {
            push_error(context.state, "Usage: /logout");
        } else {
            *context.input_page = Some(InputPageSession::authentication(None, true));
            outcome.outbound.push(AgentRequest::AuthGet {
                provider_ref: None,
                logout: true,
            });
        }
        return outcome;
    }
    let Some(command) = builtin_command(name) else {
        // A client-only draft is not attached to an agent of its own. Never
        // let an integrated command mutate the retained old session.
        if has_new_conversation(context.state) {
            set_new_conversation_notice(
                context.state,
                tr(context.language, "command.new.must_send"),
            );
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
            if is_pi {
                let provider_ref =
                    (!raw_input.trim().is_empty()).then(|| raw_input.trim().to_owned());
                *context.input_page = Some(InputPageSession::authentication(
                    provider_ref.clone(),
                    false,
                ));
                outcome.outbound.push(AgentRequest::AuthGet {
                    provider_ref,
                    logout: false,
                });
            } else {
                if reject_arguments(&context, name, raw_input) {
                    return outcome;
                }
                *context.input_page = Some(InputPageSession::login());
                outcome.outbound.push(AgentRequest::LoginGet);
            }
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
            let reference = raw_input.trim();
            if reference.is_empty() {
                // The model picker is session-independent: providers and models
                // come from the host catalog, and a pending selection is applied
                // to the next materialized session (including a deferred `/new`).
                *context.input_page = Some(InputPageSession::model());
                outcome.outbound.push(AgentRequest::ModelGet);
            } else {
                let parts: Vec<_> = reference.split_whitespace().collect();
                let default_effort = match parts.as_slice() {
                    [_] => None,
                    [_, "set-default-effort", effort] => Some(*effort),
                    _ => {
                        push_error(context.state, tr(context.language, "command.model.usage"));
                        return outcome;
                    }
                };
                let Some((provider, model)) =
                    resolve_model_reference(context.model_providers, parts[0])
                else {
                    push_error(
                        context.state,
                        tr_args(
                            context.language,
                            "command.model.not_found",
                            &[("reference", parts[0].to_owned())],
                        ),
                    );
                    return outcome;
                };
                if let Some(reference) = default_effort {
                    let effort = model.reasoning.as_ref().and_then(|reasoning| {
                        reasoning
                            .efforts
                            .iter()
                            .find(|effort| effort.id.eq_ignore_ascii_case(reference))
                    });
                    if let Some(effort) = effort {
                        context.config.model_default_efforts.set(
                            &provider.id,
                            &model.id,
                            &effort.id,
                        );
                        outcome.config_changed = true;
                    } else {
                        push_error(
                            context.state,
                            tr_args(
                                context.language,
                                "command.model.effort_not_found",
                                &[
                                    ("reference", reference.to_owned()),
                                    ("model", parts[0].to_owned()),
                                ],
                            ),
                        );
                    }
                } else {
                    outcome.outbound.push(AgentRequest::ModelSet {
                        provider: provider.id.clone(),
                        model: model.id.clone(),
                        reasoning_effort: crate::catalog::configured_model_effort(
                            &context.config.model_default_efforts,
                            &provider.id,
                            model,
                        ),
                    });
                }
            }
        }
        CommandAction::Compact => {
            let args = raw_input.trim();
            let mut parts = args.split_whitespace();
            match parts.next() {
                Some("set-model") => {
                    let reference = parts.next();
                    if parts.next().is_some() {
                        push_error(context.state, tr(context.language, "command.compact.usage"));
                    } else if let Some(reference) = reference {
                        if let Some((provider, model)) =
                            resolve_model_reference(context.model_providers, reference)
                        {
                            forward(
                                format!("/compact set-model {}/{}", provider.id, model.id),
                                &mut outcome,
                                false,
                            );
                        } else {
                            push_error(
                                context.state,
                                tr_args(
                                    context.language,
                                    "command.model.not_found",
                                    &[("reference", reference.to_owned())],
                                ),
                            );
                        }
                    } else {
                        *context.input_page = Some(InputPageSession::compaction_model());
                        outcome.outbound.push(AgentRequest::ModelGet);
                    }
                }
                Some("unset-model") => {
                    if parts.next().is_some() {
                        push_error(context.state, tr(context.language, "command.compact.usage"));
                    } else {
                        forward("/compact unset-model".into(), &mut outcome, false);
                    }
                }
                _ => {
                    if has_new_conversation(context.state) {
                        set_new_conversation_notice(
                            context.state,
                            tr(context.language, "command.new.must_send"),
                        );
                    } else {
                        forward(line, &mut outcome, true);
                    }
                }
            }
        }
        CommandAction::Effort => {
            let reference = raw_input.trim();
            if reference.is_empty() {
                // The effort picker reads the exact current route's adapter-declared
                // efforts from the same model catalog; the selected effort is applied
                // to the materialized session (including a deferred `/new`).
                *context.input_page = Some(InputPageSession::effort());
                outcome.outbound.push(AgentRequest::ModelGet);
            } else if reference.split_whitespace().count() != 1 {
                push_error(context.state, tr(context.language, "command.effort.usage"));
            } else if let Some((provider, model, reasoning_effort)) =
                resolve_effort_reference(context.model_providers, context.current_model, reference)
            {
                outcome.outbound.push(AgentRequest::ModelSet {
                    provider,
                    model,
                    reasoning_effort: Some(reasoning_effort),
                });
            } else {
                push_error(
                    context.state,
                    tr_args(
                        context.language,
                        "command.effort.not_found",
                        &[("reference", reference.to_owned())],
                    ),
                );
            }
        }
        CommandAction::Reload => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            outcome.reload_config = true;
            forward("/reload".into(), &mut outcome, false);
        }
        CommandAction::Econfig => {
            if !reject_arguments(&context, name, raw_input) {
                context
                    .state
                    .lock()
                    .unwrap()
                    .push_system_message(context.config.config_path_display.clone());
            }
        }
        CommandAction::Copy => {
            if reject_arguments(&context, name, raw_input) {
                return outcome;
            }
            outcome.copy_markdown = context
                .state
                .lock()
                .unwrap()
                .latest_completed_assistant_markdown()
                .map(str::to_owned);
            if outcome.copy_markdown.is_none() {
                push_error(
                    context.state,
                    tr(context.language, "command.copy.unavailable"),
                );
            }
        }
        CommandAction::Help => {
            if !reject_arguments(&context, name, raw_input) {
                outcome.open_help = true;
            }
        }
        CommandAction::Reading => {
            if !reject_arguments(&context, name, raw_input) {
                outcome.activate_reading = true;
            }
        }
        CommandAction::History => {
            if context.question_open || context.approval_open {
                push_error(
                    context.state,
                    tr(context.language, "command.history.blocked"),
                );
            } else if let Some(action) = resolve_fixed_subcommand(command, raw_input) {
                outcome.history = Some(action);
            } else {
                push_error(context.state, tr(context.language, "command.history.usage"));
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
                push_error(context.state, tr(context.language, "command.resume.usage"));
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
                push_error(context.state, tr(context.language, "command.new.blocked"));
            } else if materializing {
                set_new_conversation_notice(
                    context.state,
                    tr(context.language, "command.new.in_progress"),
                );
            } else if let Some(line) = new_command_line(raw_input, &context.config.default_mode) {
                let mode = line.trim_start_matches("/new ").to_owned();
                context.state.lock().unwrap().begin_new_conversation(mode);
                outcome.new_conversation = true;
            } else {
                push_error(context.state, tr(context.language, "command.new.usage"));
            }
        }
        // Bridge-optimized commands and known DSH commands still use the
        // command plane; their declaration remains local so richer argument
        // completion can be added without duplicating metadata elsewhere.
        CommandAction::Skill | CommandAction::Forward => {
            if has_new_conversation(context.state) {
                set_new_conversation_notice(
                    context.state,
                    tr(context.language, "command.new.must_send"),
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplayItem;

    #[test]
    fn skill_can_be_the_first_submission_with_immediate_feedback() {
        for drafting in [false, true] {
            for line in ["/skill:review", "/skill review"] {
                let state = Arc::new(Mutex::new(RuntimeState::default()));
                if drafting {
                    state.lock().unwrap().begin_new_conversation("standard");
                }
                let mut input_page = None;
                let mut config = Config::default();
                let mut paste = config.paste_placeholder_chars;
                let mut history = config.history_limit;
                let mut theme = config.theme();
                let outcome = handle_local_command(
                    line.into(),
                    LocalCommandContext {
                        language: config.language,
                        input_page: &mut input_page,
                        integrated_commands: &[],
                        config: &mut config,
                        themes: &mut Vec::new(),
                        new_modes: &[],
                        model_providers: &[],
                        current_model: None,
                        input_paste_placeholder_chars: &mut paste,
                        input_history_limit: &mut history,
                        theme: &mut theme,
                        question_open: false,
                        approval_open: false,
                        state: &state,
                    },
                );
                assert!(!outcome.starts_interruptible_command);
                let app = state.lock().unwrap();
                if drafting {
                    assert!(
                        matches!(outcome.outbound.as_slice(), [AgentRequest::NewInput { prompt, .. }] if prompt == &line)
                    );
                    assert_eq!(
                        app.session
                            .new_conversation
                            .as_ref()
                            .unwrap()
                            .pending_card
                            .as_ref()
                            .unwrap()
                            .content,
                        "review"
                    );
                } else {
                    assert!(matches!(
                        outcome.outbound.as_slice(),
                        [AgentRequest::Command { .. }]
                    ));
                    assert!(
                        matches!(&app.transcript.nodes()[0].item, DisplayItem::Card(card) if card.content == "review")
                    );
                    assert!(app.session.working);
                }
            }
        }
    }

    #[test]
    fn help_requests_the_shared_modal_without_mutating_the_transcript() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut input_page = None;
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut paste_placeholder_chars = config.paste_placeholder_chars;
        let mut history_limit = config.history_limit;
        let mut theme = config.theme();

        let outcome = handle_local_command(
            "/help".into(),
            LocalCommandContext {
                language: config.language,
                input_page: &mut input_page,
                integrated_commands: &[CommandDescriptor {
                    name: "feedback".into(),
                    description: "record feedback".into(),
                    input_hint: Some("<text>".into()),
                }],
                config: &mut config,
                themes: &mut themes,
                new_modes: &[],
                model_providers: &[],
                current_model: None,
                input_paste_placeholder_chars: &mut paste_placeholder_chars,
                input_history_limit: &mut history_limit,
                theme: &mut theme,
                question_open: false,
                approval_open: false,
                state: &state,
            },
        );

        assert!(outcome.open_help);
        assert!(outcome.outbound.is_empty());
        assert!(state.lock().unwrap().transcript.nodes().is_empty());
    }

    #[test]
    fn econfig_prints_adapter_path_without_an_agent_request() {
        for (line, valid) in [("/econfig", true), ("/econfig extra", false)] {
            let state = Arc::new(Mutex::new(RuntimeState::default()));
            let mut input_page = None;
            let mut config = Config {
                config_path_display: "C:\\用户\\e\\config.toml".into(),
                ..Config::default()
            };
            let expected = config.config_path_display.clone();
            let mut themes = Vec::new();
            let mut paste_placeholder_chars = config.paste_placeholder_chars;
            let mut history_limit = config.history_limit;
            let mut theme = config.theme();
            let outcome = handle_local_command(
                line.into(),
                LocalCommandContext {
                    language: config.language,
                    input_page: &mut input_page,
                    integrated_commands: &[],
                    config: &mut config,
                    themes: &mut themes,
                    new_modes: &[],
                    model_providers: &[],
                    current_model: None,
                    input_paste_placeholder_chars: &mut paste_placeholder_chars,
                    input_history_limit: &mut history_limit,
                    theme: &mut theme,
                    question_open: false,
                    approval_open: false,
                    state: &state,
                },
            );
            assert!(outcome.outbound.is_empty());
            assert!(input_page.is_none());
            let state = state.lock().unwrap();
            let DisplayItem::Block(block) = &state.transcript.nodes().last().unwrap().item else {
                panic!("econfig must append a transcript block");
            };
            assert_eq!(block.content == expected, valid);
            assert!(!block.streaming);
        }
    }

    #[test]
    fn copy_without_source_or_with_arguments_reports_a_local_error() {
        for (line, expected) in [
            (
                "/copy",
                "No completed assistant Markdown response is available to copy",
            ),
            ("/copy extra", "Usage: /copy"),
        ] {
            let state = Arc::new(Mutex::new(RuntimeState::default()));
            let mut input_page = None;
            let mut config = Config::default();
            let mut paste = config.paste_placeholder_chars;
            let mut history = config.history_limit;
            let mut theme = config.theme();
            let outcome = handle_local_command(
                line.into(),
                LocalCommandContext {
                    language: config.language,
                    input_page: &mut input_page,
                    integrated_commands: &[],
                    config: &mut config,
                    themes: &mut Vec::new(),
                    new_modes: &[],
                    model_providers: &[],
                    current_model: None,
                    input_paste_placeholder_chars: &mut paste,
                    input_history_limit: &mut history,
                    theme: &mut theme,
                    question_open: false,
                    approval_open: false,
                    state: &state,
                },
            );
            assert!(outcome.copy_markdown.is_none());
            assert!(outcome.outbound.is_empty());
            let app = state.lock().unwrap();
            assert!(matches!(
                &app.transcript.nodes().last().unwrap().item,
                DisplayItem::Block(block) if block.copy_source == expected
            ));
        }
    }

    #[test]
    fn history_default_and_fixed_actions_dispatch_locally_and_reject_extras() {
        for (line, expected) in [
            ("/history", Some(FixedSubcommandAction::HistoryShow)),
            ("/history show", Some(FixedSubcommandAction::HistoryShow)),
            ("/history path", Some(FixedSubcommandAction::HistoryPath)),
            ("/history copy", Some(FixedSubcommandAction::HistoryCopy)),
            (
                "/history copy-10",
                Some(FixedSubcommandAction::HistoryCopy10),
            ),
            ("/history missing", None),
            ("/history copy extra", None),
        ] {
            let state = Arc::new(Mutex::new(RuntimeState::default()));
            let mut input_page = None;
            let mut config = Config::default();
            let mut themes = Vec::new();
            let mut paste = config.paste_placeholder_chars;
            let mut input_history = config.history_limit;
            let mut theme = config.theme();
            let outcome = handle_local_command(
                line.into(),
                LocalCommandContext {
                    language: config.language,
                    input_page: &mut input_page,
                    integrated_commands: &[],
                    config: &mut config,
                    themes: &mut themes,
                    new_modes: &[],
                    model_providers: &[],
                    current_model: None,
                    input_paste_placeholder_chars: &mut paste,
                    input_history_limit: &mut input_history,
                    theme: &mut theme,
                    question_open: false,
                    approval_open: false,
                    state: &state,
                },
            );
            assert_eq!(outcome.history, expected, "{line}");
            assert!(outcome.outbound.is_empty(), "{line}");
            if expected.is_none() {
                assert!(
                    !state.lock().unwrap().transcript.nodes().is_empty(),
                    "{line}"
                );
            }
        }
    }

    fn model_provider(id: &str, model_ids: &[&str]) -> ModelProvider {
        ModelProvider {
            id: id.into(),
            name: id.into(),
            models: model_ids
                .iter()
                .map(|model| crate::agent::ModelDescriptor {
                    id: (*model).into(),
                    name: (*model).into(),
                    description: None,
                    context_window: None,
                    reasoning: None,
                })
                .collect(),
        }
    }

    #[test]
    fn model_argument_selects_canonical_or_unique_bare_reference() {
        let providers = vec![
            model_provider("anthropic", &["claude-sonnet", "shared"]),
            model_provider("openrouter", &["shared"]),
        ];
        assert_eq!(
            resolve_model_reference(&providers, "anthropic/claude-sonnet")
                .map(|(p, m)| (p.id.as_str(), m.id.as_str())),
            Some(("anthropic", "claude-sonnet"))
        );
        assert_eq!(
            resolve_model_reference(&providers, "claude-sonnet")
                .map(|(p, m)| (p.id.as_str(), m.id.as_str())),
            Some(("anthropic", "claude-sonnet"))
        );
        assert_eq!(resolve_model_reference(&providers, "shared"), None);

        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut input_page = None;
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut paste_placeholder_chars = config.paste_placeholder_chars;
        let mut history_limit = config.history_limit;
        let mut theme = config.theme();
        let outcome = handle_local_command(
            "/model anthropic/claude-sonnet".into(),
            LocalCommandContext {
                language: config.language,
                input_page: &mut input_page,
                integrated_commands: &[],
                config: &mut config,
                themes: &mut themes,
                new_modes: &[],
                model_providers: &providers,
                current_model: None,
                input_paste_placeholder_chars: &mut paste_placeholder_chars,
                input_history_limit: &mut history_limit,
                theme: &mut theme,
                question_open: false,
                approval_open: false,
                state: &state,
            },
        );
        assert!(input_page.is_none());
        assert!(matches!(
            outcome.outbound.as_slice(),
            [AgentRequest::ModelSet {
                provider,
                model,
                reasoning_effort: None,
            }] if provider == "anthropic" && model == "claude-sonnet"
        ));
    }

    #[test]
    fn model_default_effort_command_validates_route_and_never_calls_backend() {
        let mut providers = vec![
            model_provider("p", &["m", "shared", "plain"]),
            model_provider("q", &["shared"]),
        ];
        providers[0].models[0].reasoning = Some(crate::agent::ModelReasoning {
            efforts: vec![crate::agent::ReasoningEffort {
                id: "high".into(),
                name: "High".into(),
                description: None,
            }],
            default_effort: None,
        });
        for (line, valid) in [
            ("/model p/m set-default-effort high", true),
            ("/model M set-default-effort HIGH", true),
            ("/model shared set-default-effort high", false),
            ("/model p/plain set-default-effort high", false),
            ("/model missing set-default-effort high", false),
            ("/model p/m set-default-effort low", false),
            ("/model p/m set-default-effort", false),
            ("/model p/m set-default-effort high extra", false),
            ("/model p/m unknown high", false),
        ] {
            let state = Arc::new(Mutex::new(RuntimeState::default()));
            let mut config = Config::default();
            config.model_default_efforts.set("q", "shared", "low");
            let mut page = None;
            let mut paste = config.paste_placeholder_chars;
            let mut history = config.history_limit;
            let mut theme = config.theme();
            let outcome = handle_local_command(
                line.into(),
                LocalCommandContext {
                    language: config.language,
                    input_page: &mut page,
                    integrated_commands: &[],
                    config: &mut config,
                    themes: &mut Vec::new(),
                    new_modes: &[],
                    model_providers: &providers,
                    current_model: None,
                    input_paste_placeholder_chars: &mut paste,
                    input_history_limit: &mut history,
                    theme: &mut theme,
                    question_open: false,
                    approval_open: false,
                    state: &state,
                },
            );
            assert!(outcome.outbound.is_empty(), "{line}");
            assert!(!outcome.starts_interruptible_command);
            assert_eq!(outcome.config_changed, valid, "{line}");
            assert_eq!(
                config.model_default_efforts.get("p", "m"),
                valid.then_some("high")
            );
            assert_eq!(config.model_default_efforts.get("q", "shared"), Some("low"));
            assert_eq!(
                state.lock().unwrap().transcript.nodes().is_empty(),
                valid,
                "{line}"
            );
        }
    }

    #[test]
    fn compaction_model_commands_reuse_catalog_without_selecting_chat_model() {
        let providers = vec![
            model_provider("p", &["small", "shared"]),
            model_provider("q", &["shared"]),
        ];
        for line in [
            "/compact set-model small",
            "/compact set-model",
            "/compact unset-model",
            "/compact set-model shared",
            "/compact unset-model extra",
            "/reload",
        ] {
            let state = Arc::new(Mutex::new(RuntimeState::default()));
            let mut input_page = None;
            let mut config = Config::default();
            let mut paste = config.paste_placeholder_chars;
            let mut history = config.history_limit;
            let mut theme = config.theme();
            let outcome = handle_local_command(
                line.into(),
                LocalCommandContext {
                    language: config.language,
                    input_page: &mut input_page,
                    integrated_commands: &[],
                    config: &mut config,
                    themes: &mut Vec::new(),
                    new_modes: &[],
                    model_providers: &providers,
                    current_model: None,
                    input_paste_placeholder_chars: &mut paste,
                    input_history_limit: &mut history,
                    theme: &mut theme,
                    question_open: false,
                    approval_open: false,
                    state: &state,
                },
            );
            assert!(!outcome.starts_interruptible_command);
            match line {
                "/reload" => {
                    assert!(outcome.reload_config);
                    assert!(
                        matches!(&outcome.outbound[..], [AgentRequest::Command { line, .. }] if line == "/reload")
                    );
                }
                "/compact set-model small" => assert!(
                    matches!(&outcome.outbound[..], [AgentRequest::Command { line, .. }] if line == "/compact set-model p/small")
                ),
                "/compact unset-model" => assert!(
                    matches!(&outcome.outbound[..], [AgentRequest::Command { line, .. }] if line == "/compact unset-model")
                ),
                "/compact set-model" => {
                    assert_eq!(outcome.outbound, vec![AgentRequest::ModelGet]);
                    let page = input_page.unwrap();
                    assert!(
                        matches!(page.page, crate::input_page::InputPage::Model(model) if model.for_compaction)
                    );
                }
                _ => assert!(outcome.outbound.is_empty()),
            }
        }
    }

    #[test]
    fn compaction_model_configuration_works_while_a_new_conversation_is_drafted() {
        let providers = vec![model_provider("p", &["small"])];
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        state.lock().unwrap().begin_new_conversation("standard");
        let mut config = Config::default();
        let language = config.language;
        let mut paste = config.paste_placeholder_chars;
        let mut history = config.history_limit;
        let mut theme = config.theme();
        let mut input_page = None;
        let mut run = |line: &str, input_page: &mut Option<InputPageSession>| {
            handle_local_command(
                line.into(),
                LocalCommandContext {
                    language,
                    input_page,
                    integrated_commands: &[],
                    config: &mut config,
                    themes: &mut Vec::new(),
                    new_modes: &[],
                    model_providers: &providers,
                    current_model: None,
                    input_paste_placeholder_chars: &mut paste,
                    input_history_limit: &mut history,
                    theme: &mut theme,
                    question_open: false,
                    approval_open: false,
                    state: &state,
                },
            )
        };
        let notice = |state: &Arc<Mutex<RuntimeState>>| {
            state
                .lock()
                .unwrap()
                .session
                .new_conversation
                .as_ref()
                .and_then(|draft| draft.notice.clone())
        };

        let outcome = run("/compact set-model small", &mut input_page);
        assert!(matches!(
            &outcome.outbound[..],
            [AgentRequest::Command { line, .. }] if line == "/compact set-model p/small"
        ));
        assert_eq!(notice(&state), None);

        // A bare `/compact` still needs the materialized session.
        let outcome = run("/compact", &mut input_page);
        assert!(outcome.outbound.is_empty());
        assert_eq!(
            notice(&state).as_deref(),
            Some(tr(language, "command.new.must_send").as_str())
        );
    }

    #[test]
    fn effort_argument_selects_current_models_declared_effort() {
        let current = ModelSelection {
            provider: "openai".into(),
            model: "gpt".into(),
            reasoning_effort: Some("low".into()),
        };
        let providers = vec![ModelProvider {
            id: "openai".into(),
            name: "OpenAI".into(),
            models: vec![crate::agent::ModelDescriptor {
                id: "gpt".into(),
                name: "GPT".into(),
                description: None,
                context_window: None,
                reasoning: Some(crate::agent::ModelReasoning {
                    efforts: vec![crate::agent::ReasoningEffort {
                        id: "high".into(),
                        name: "High".into(),
                        description: None,
                    }],
                    default_effort: None,
                }),
            }],
        }];
        assert_eq!(
            resolve_effort_reference(&providers, Some(&current), "HIGH"),
            Some(("openai".into(), "gpt".into(), "high".into()))
        );

        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut input_page = None;
        let mut config = Config::default();
        let mut themes = Vec::new();
        let mut paste_placeholder_chars = config.paste_placeholder_chars;
        let mut history_limit = config.history_limit;
        let mut theme = config.theme();
        let outcome = handle_local_command(
            "/effort high".into(),
            LocalCommandContext {
                language: config.language,
                input_page: &mut input_page,
                integrated_commands: &[],
                config: &mut config,
                themes: &mut themes,
                new_modes: &[],
                model_providers: &providers,
                current_model: Some(&current),
                input_paste_placeholder_chars: &mut paste_placeholder_chars,
                input_history_limit: &mut history_limit,
                theme: &mut theme,
                question_open: false,
                approval_open: false,
                state: &state,
            },
        );
        assert!(input_page.is_none());
        assert!(matches!(
            outcome.outbound.as_slice(),
            [AgentRequest::ModelSet {
                provider,
                model,
                reasoning_effort: Some(effort),
            }] if provider == "openai" && model == "gpt" && effort == "high"
        ));
    }

    #[test]
    fn pi_login_and_logout_are_local_authentication_commands() {
        for (line, expected_ref, logout) in [
            ("/login", None, false),
            ("/login OpenAI", Some("OpenAI"), false),
            ("/login Google Gemini", Some("Google Gemini"), false),
            ("/logout", None, true),
        ] {
            let state = Arc::new(Mutex::new(RuntimeState::default()));
            state.lock().unwrap().frontend = crate::FrontendKind::Pi;
            let mut input_page = None;
            let mut config = Config::default();
            let mut themes = Vec::new();
            let mut paste = config.paste_placeholder_chars;
            let mut history = config.history_limit;
            let mut theme = config.theme();
            let outcome = handle_local_command(
                line.into(),
                LocalCommandContext {
                    language: config.language,
                    input_page: &mut input_page,
                    integrated_commands: &[],
                    config: &mut config,
                    themes: &mut themes,
                    new_modes: &[],
                    model_providers: &[],
                    current_model: None,
                    input_paste_placeholder_chars: &mut paste,
                    input_history_limit: &mut history,
                    theme: &mut theme,
                    question_open: false,
                    approval_open: false,
                    state: &state,
                },
            );
            assert!(matches!(
                outcome.outbound.as_slice(),
                [AgentRequest::AuthGet { provider_ref, logout: actual_logout }]
                    if provider_ref.as_deref() == expected_ref && *actual_logout == logout
            ));
            assert!(matches!(
                input_page,
                Some(InputPageSession {
                    page: crate::input_page::InputPage::Login(_),
                    ..
                })
            ));
        }
    }
}
