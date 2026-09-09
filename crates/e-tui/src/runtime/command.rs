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
    command_catalog::{resolve_fixed_subcommand, CommandAction, NewMode},
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
    pub new_conversation: bool,
    pub activate_reading: bool,
    pub history: Option<FixedSubcommandAction>,
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

fn resolve_model_reference(
    providers: &[ModelProvider],
    reference: &str,
) -> Option<(String, String)> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }

    let mut canonical = providers.iter().flat_map(|provider| {
        provider.models.iter().filter_map(move |model| {
            format!("{}/{}", provider.id, model.id)
                .eq_ignore_ascii_case(reference)
                .then(|| (provider.id.clone(), model.id.clone()))
        })
    });
    match (canonical.next(), canonical.next()) {
        (Some(route), None) => return Some(route),
        (Some(_), Some(_)) => return None,
        (None, _) => {}
    }

    let mut bare = providers.iter().flat_map(|provider| {
        provider.models.iter().filter_map(move |model| {
            model
                .id
                .eq_ignore_ascii_case(reference)
                .then(|| (provider.id.clone(), model.id.clone()))
        })
    });
    match (bare.next(), bare.next()) {
        (Some(route), None) => Some(route),
        _ => None,
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
            let reference = raw_input.trim();
            if reference.is_empty() {
                // The model picker is session-independent: providers and models
                // come from the host catalog, and a pending selection is applied
                // to the next materialized session (including a deferred `/new`).
                *context.input_page = Some(InputPageSession::model());
                outcome.outbound.push(AgentRequest::ModelGet);
            } else if reference.split_whitespace().count() != 1 {
                push_error(context.state, tr(context.language, "command.model.usage"));
            } else if let Some((provider, model)) =
                resolve_model_reference(context.model_providers, reference)
            {
                outcome.outbound.push(AgentRequest::ModelSet {
                    provider,
                    model,
                    reasoning_effort: None,
                });
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
        }
        CommandAction::Compact => {
            if has_new_conversation(context.state) {
                set_new_conversation_notice(
                    context.state,
                    tr(context.language, "command.new.must_send"),
                );
                return outcome;
            }
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
                                format!("/compact set-model {provider}/{model}"),
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
                _ => forward(line, &mut outcome, true),
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
        CommandAction::Help => {
            if !reject_arguments(&context, name, raw_input) {
                let markdown = crate::help::markdown(context.config, context.integrated_commands);
                context.state.lock().unwrap().push_local_markdown(markdown);
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
    use crate::display::{DisplayItem, TranscriptFormat};

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
    fn help_appends_local_markdown_without_an_agent_request() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let integrated = vec![
            CommandDescriptor {
                name: "feedback".into(),
                description: "record feedback".into(),
                input_hint: Some("<text>".into()),
            },
            CommandDescriptor {
                name: "plan".into(),
                description: "shadowed host plan".into(),
                input_hint: None,
            },
        ];
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
                integrated_commands: &integrated,
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
        let state = state.lock().unwrap();
        assert!(!state.interaction.help_visible);
        let DisplayItem::Block(block) = &state.transcript.nodes().last().unwrap().item else {
            panic!("help must append a transcript block");
        };
        assert_eq!(block.format, TranscriptFormat::Markdown);
        assert!(!block.streaming);
        assert!(block.unit.is_none(), "Markdown owns provenance allocation");
        assert!(block.content.contains("# e help"));
        assert!(block
            .content
            .contains("`/feedback`: record feedback <text>"));
        assert_eq!(block.content.matches("`/plan`").count(), 1);
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
            resolve_model_reference(&providers, "anthropic/claude-sonnet"),
            Some(("anthropic".into(), "claude-sonnet".into()))
        );
        assert_eq!(
            resolve_model_reference(&providers, "claude-sonnet"),
            Some(("anthropic".into(), "claude-sonnet".into()))
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
}
