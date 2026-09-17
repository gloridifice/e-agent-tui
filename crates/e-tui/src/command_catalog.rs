//! Pure slash-command catalog and completion policy.
//!
//! This leaf module owns built-in declarations, integrated-command merging,
//! fuzzy ranking, and argument-completion metadata. It does not depend on
//! input state, application state, pages, copy mode, or async senders.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDescriptor {
    pub name: String,
    pub description: String,
    pub input_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMode {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    None,
    NewMode,
    Model,
    Compact,
    Effort,
    Skill,
    FixedSubcommands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAction {
    Settings,
    Login,
    New,
    Resume,
    Model,
    Compact,
    Effort,
    Theme,
    Reload,
    Econfig,
    Copy,
    Skill,
    Forward,
    Help,
    Reading,
    History,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedSubcommandAction {
    HistoryShow,
    HistoryPath,
    HistoryCopy,
    HistoryCopy10,
}

#[derive(Debug, Clone, Copy)]
pub struct FixedSubcommand {
    pub name: &'static str,
    pub description_key: &'static str,
    pub action: FixedSubcommandAction,
}

pub const HISTORY_SUBCOMMANDS: &[FixedSubcommand] = &[
    FixedSubcommand {
        name: "show",
        description_key: "command.history.show.description",
        action: FixedSubcommandAction::HistoryShow,
    },
    FixedSubcommand {
        name: "path",
        description_key: "command.history.path.description",
        action: FixedSubcommandAction::HistoryPath,
    },
    FixedSubcommand {
        name: "copy",
        description_key: "command.history.copy.description",
        action: FixedSubcommandAction::HistoryCopy,
    },
    FixedSubcommand {
        name: "copy-10",
        description_key: "command.history.copy_10.description",
        action: FixedSubcommandAction::HistoryCopy10,
    },
];

/// One optimized command. `name` never includes the leading slash.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinCommand {
    pub name: &'static str,
    pub description_key: &'static str,
    pub input_hint_key: Option<&'static str>,
    pub completion: CompletionKind,
    pub action: CommandAction,
    pub subcommands: &'static [FixedSubcommand],
    pub default_subcommand: Option<FixedSubcommandAction>,
}

macro_rules! command {
    ($name:literal, $description_key:literal, $hint_key:expr, $completion:ident, $action:ident) => {
        BuiltinCommand {
            name: $name,
            description_key: $description_key,
            input_hint_key: $hint_key,
            completion: CompletionKind::$completion,
            action: CommandAction::$action,
            subcommands: &[],
            default_subcommand: None,
        }
    };
}

/// The sole declaration site for built-in behavior and completion metadata.
pub const BUILTIN_COMMANDS: &[BuiltinCommand] = &[
    command!("help", "command.help.description", None, None, Help),
    command!(
        "settings",
        "command.settings.description",
        None,
        None,
        Settings
    ),
    command!("login", "command.login.description", None, None, Login),
    command!(
        "new",
        "command.new.description",
        Some("command.new.hint"),
        NewMode,
        New
    ),
    command!(
        "resume",
        "command.resume.description",
        Some("command.resume.hint"),
        None,
        Resume
    ),
    command!(
        "model",
        "command.model.description",
        Some("command.model.hint"),
        Model,
        Model
    ),
    command!(
        "effort",
        "command.effort.description",
        Some("command.effort.hint"),
        Effort,
        Effort
    ),
    command!("theme", "command.theme.description", None, None, Theme),
    command!("reload", "command.reload.description", None, None, Reload),
    command!(
        "econfig",
        "command.econfig.description",
        None,
        None,
        Econfig
    ),
    command!("copy", "command.copy.description", None, None, Copy),
    command!(
        "skill",
        "command.skill.description",
        Some("command.skill.hint"),
        Skill,
        Skill
    ),
    command!(
        "compact",
        "command.compact.description",
        Some("command.compact.hint"),
        Compact,
        Compact
    ),
    command!(
        "goal",
        "command.goal.description",
        Some("command.goal.hint"),
        None,
        Forward
    ),
    command!(
        "plan",
        "command.plan.description",
        Some("command.plan.hint"),
        None,
        Forward
    ),
    command!("read", "command.read.description", None, None, Reading),
    BuiltinCommand {
        name: "history",
        description_key: "command.history.description",
        input_hint_key: Some("command.history.hint"),
        completion: CompletionKind::FixedSubcommands,
        action: CommandAction::History,
        subcommands: HISTORY_SUBCOMMANDS,
        default_subcommand: Some(FixedSubcommandAction::HistoryShow),
    },
    command!("exit", "command.quit.description", None, None, Quit),
    command!("q", "command.quit.description", None, None, Quit),
    command!("quit", "command.quit.description", None, None, Quit),
];

pub fn builtin_command(name: &str) -> Option<&'static BuiltinCommand> {
    BUILTIN_COMMANDS.iter().find(|command| command.name == name)
}

/// Resolve an argument-completion context from the central declaration.
pub fn match_fixed_subcommands(
    command: &'static BuiltinCommand,
    query: &str,
) -> Vec<&'static FixedSubcommand> {
    let query = query.to_lowercase();
    rank_candidates(&query, command.subcommands.iter(), |query, subcommand| {
        rank_text(query, &subcommand.name.to_lowercase())
    })
}

pub fn resolve_fixed_subcommand(
    command: &'static BuiltinCommand,
    input: &str,
) -> Option<FixedSubcommandAction> {
    let mut words = input.split_whitespace();
    let first = words.next();
    if words.next().is_some() {
        return None;
    }
    match first {
        None => command.default_subcommand,
        Some(name) => command
            .subcommands
            .iter()
            .find(|subcommand| subcommand.name == name)
            .map(|subcommand| subcommand.action),
    }
}

pub fn completion_context(line: &str) -> Option<(&'static BuiltinCommand, &str)> {
    let body = line.strip_prefix('/')?;
    let skill = builtin_command("skill").expect("skill command is registered");
    if body == skill.name {
        return Some((skill, ""));
    }
    if let Some(query) = body.strip_prefix("skill:") {
        return Some((skill, query));
    }
    if let Some(query) = body.strip_prefix("skill ") {
        return Some((skill, query));
    }

    let (name, query) = body.split_once(' ')?;
    let command = builtin_command(name)?;
    (command.completion != CompletionKind::None).then_some((command, query))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    Builtin,
    Integrated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandText {
    Builtin {
        description_key: &'static str,
        input_hint_key: Option<&'static str>,
    },
    Integrated {
        description: String,
        input_hint: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCandidate {
    pub line: String,
    pub text: CommandText,
    pub source: CommandSource,
}

/// Format a command candidate without coupling the catalog to localization.
/// Built-in keys are resolved by the caller; integrated text is already local.
pub(crate) fn candidate_description(
    candidate: &CommandCandidate,
    translate: impl Fn(&str) -> String,
) -> String {
    let (description, hint) = match &candidate.text {
        CommandText::Builtin {
            description_key,
            input_hint_key,
        } => (translate(description_key), input_hint_key.map(translate)),
        CommandText::Integrated {
            description,
            input_hint,
        } => (description.clone(), input_hint.clone()),
    };
    match hint.filter(|hint| !hint.is_empty()) {
        Some(hint) => format!("{description}  {hint}"),
        None => description,
    }
}

fn is_subsequence(query: &str, name: &str) -> bool {
    let mut chars = name.chars();
    query
        .chars()
        .all(|character| chars.any(|candidate| candidate == character))
}

pub(crate) fn rank_text(query: &str, name: &str) -> Option<u8> {
    if query.is_empty() || name.starts_with(query) {
        Some(0)
    } else if name.contains(query) {
        Some(1)
    } else if is_subsequence(query, name) {
        Some(2)
    } else {
        None
    }
}

pub(crate) fn rank_fields(query: &str, fields: impl IntoIterator<Item = String>) -> Option<u8> {
    fields
        .into_iter()
        .filter_map(|field| rank_text(query, &field))
        .min()
}

pub(crate) fn rank_candidates<T, I, F>(query: &str, items: I, score: F) -> Vec<T>
where
    I: IntoIterator<Item = T>,
    F: Fn(&str, &T) -> Option<u8>,
{
    let mut groups: [Vec<T>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for item in items {
        if let Some(group) = score(query, &item) {
            if let Some(bucket) = groups.get_mut(usize::from(group)) {
                bucket.push(item);
            }
        }
    }
    groups.into_iter().flatten().collect()
}

fn rank(query: &str, name: &str) -> Option<u8> {
    rank_text(query, name)
}

/// Merge optimized commands with the current DSH registry and fuzzy-rank the
/// result. A built-in shadows a same-name DSH descriptor.
pub fn match_command_catalog(
    query: &str,
    integrated: &[CommandDescriptor],
) -> Vec<CommandCandidate> {
    let query = query.to_lowercase();
    let mut candidates: Vec<(u8, String, usize, CommandCandidate)> = Vec::new();

    for command in BUILTIN_COMMANDS {
        let name = command.name.to_lowercase();
        let Some(group) = rank(&query, &name) else {
            continue;
        };
        candidates.push((
            group,
            name,
            0,
            CommandCandidate {
                line: format!("/{}", command.name),
                text: CommandText::Builtin {
                    description_key: command.description_key,
                    input_hint_key: command.input_hint_key,
                },
                source: CommandSource::Builtin,
            },
        ));
    }

    for command in integrated {
        if builtin_command(&command.name).is_some() {
            continue;
        }
        let name = command.name.to_lowercase();
        let Some(group) = rank(&query, &name) else {
            continue;
        };
        candidates.push((
            group,
            name,
            1,
            CommandCandidate {
                line: format!("/{}", command.name),
                text: CommandText::Integrated {
                    description: command.description.clone(),
                    input_hint: command.input_hint.clone(),
                },
                source: CommandSource::Integrated,
            },
        ));
    }

    candidates.sort_by(|left, right| {
        (left.0, left.1.as_str(), left.2).cmp(&(right.0, right.1.as_str(), right.2))
    });
    candidates
        .into_iter()
        .map(|(_, _, _, candidate)| candidate)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_shadow_integrated_commands_and_keep_hints() {
        let integrated = vec![
            CommandDescriptor {
                name: "feedback".into(),
                description: "record feedback".into(),
                input_hint: Some("<text>".into()),
            },
            CommandDescriptor {
                name: "plan".into(),
                description: "host plan".into(),
                input_hint: None,
            },
        ];
        let all = match_command_catalog("", &integrated);
        assert_eq!(all.iter().filter(|item| item.line == "/plan").count(), 1);
        let feedback = all.iter().find(|item| item.line == "/feedback").unwrap();
        assert_eq!(feedback.source, CommandSource::Integrated);
        assert!(matches!(
            &feedback.text,
            CommandText::Integrated { input_hint: Some(hint), .. } if hint == "<text>"
        ));
    }

    #[test]
    fn one_entry_owns_completion_and_action() {
        let new = builtin_command("new").unwrap();
        assert_eq!(new.completion, CompletionKind::NewMode);
        assert_eq!(new.action, CommandAction::New);
        assert_eq!(completion_context("/new m").unwrap().1, "m");
        assert_eq!(completion_context("/model ").unwrap().1, "");
        assert_eq!(completion_context("/effort h").unwrap().1, "h");
        assert_eq!(completion_context("/skill").unwrap().1, "");
        let history = builtin_command("history").unwrap();
        assert_eq!(completion_context("/history ").unwrap().1, "");
        assert_eq!(
            resolve_fixed_subcommand(history, ""),
            Some(FixedSubcommandAction::HistoryShow)
        );
        assert_eq!(
            resolve_fixed_subcommand(history, "show"),
            Some(FixedSubcommandAction::HistoryShow)
        );
        assert_eq!(
            resolve_fixed_subcommand(history, " copy-10  "),
            Some(FixedSubcommandAction::HistoryCopy10)
        );
        assert_eq!(resolve_fixed_subcommand(history, "copy extra"), None);
        assert_eq!(
            match_fixed_subcommands(history, "cop")
                .iter()
                .map(|item| item.name)
                .collect::<Vec<_>>(),
            ["copy", "copy-10"]
        );
    }

    #[test]
    fn fuzzy_order_is_prefix_then_substring_then_subsequence() {
        assert_eq!(match_command_catalog("set", &[])[0].line, "/settings");
        assert_eq!(match_command_catalog("ett", &[])[0].line, "/settings");
        assert_eq!(match_command_catalog("pln", &[])[0].line, "/plan");
    }
}
