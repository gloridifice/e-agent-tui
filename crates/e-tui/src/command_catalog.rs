//! Pure slash-command catalog and completion policy.
//!
//! This leaf module owns built-in declarations, integrated-command merging,
//! fuzzy ranking, and argument-completion metadata. It does not depend on
//! input state, application state, pages, copy mode, or async senders.

use crate::agent::CommandDescriptor;

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
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAction {
    Settings,
    Login,
    New,
    Resume,
    Model,
    Theme,
    Reload,
    Skill,
    Forward,
    Help,
    Reading,
    Quit,
}

/// One optimized command. `name` never includes the leading slash.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub input_hint: Option<&'static str>,
    pub completion: CompletionKind,
    pub action: CommandAction,
}

macro_rules! command {
    ($name:literal, $description:literal, $hint:expr, $completion:ident, $action:ident) => {
        BuiltinCommand {
            name: $name,
            description: $description,
            input_hint: $hint,
            completion: CompletionKind::$completion,
            action: CommandAction::$action,
        }
    };
}

/// The sole declaration site for built-in behavior and completion metadata.
pub const BUILTIN_COMMANDS: &[BuiltinCommand] = &[
    command!("help", "显示帮助", None, None, Help),
    command!("settings", "打开设置面板", None, None, Settings),
    command!(
        "login",
        "登录设置（API key / 账号 / proxy）",
        None,
        None,
        Login
    ),
    command!("new", "新建会话", Some("[模式]"), NewMode, New),
    command!("resume", "切换或续接会话", Some("[会话 ID]"), None, Resume),
    command!("model", "选择模型（provider × model）", None, None, Model),
    command!("theme", "切换主题", None, None, Theme),
    command!("reload", "重载配置 / 主题 / 技能", None, None, Reload),
    command!("skill", "注入技能", Some("<名称>"), Skill, Skill),
    command!("compact", "压缩上下文", None, None, Forward),
    command!(
        "goal",
        "目标管理",
        Some("[目标|clear|edit|pause|resume]"),
        None,
        Forward
    ),
    command!("plan", "计划模式", Some("[off|消息]"), None, Forward),
    command!("read", "进入阅读视图", None, None, Reading),
    command!("exit", "退出客户端", None, None, Quit),
    command!("q", "退出客户端", None, None, Quit),
    command!("quit", "退出客户端", None, None, Quit),
];

pub fn builtin_command(name: &str) -> Option<&'static BuiltinCommand> {
    BUILTIN_COMMANDS.iter().find(|command| command.name == name)
}

/// Resolve an argument-completion context from the central declaration.
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
pub struct CommandCandidate {
    pub line: String,
    pub description: String,
    pub source: CommandSource,
}

fn is_subsequence(query: &str, name: &str) -> bool {
    let mut chars = name.chars();
    query
        .chars()
        .all(|character| chars.any(|candidate| candidate == character))
}

fn rank(query: &str, name: &str) -> Option<u8> {
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

fn discovery_description(description: &str, hint: Option<&str>) -> String {
    match hint.filter(|hint| !hint.is_empty()) {
        Some(hint) => format!("{description}  {hint}"),
        None => description.to_owned(),
    }
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
                description: discovery_description(command.description, command.input_hint),
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
                description: discovery_description(
                    &command.description,
                    command.input_hint.as_deref(),
                ),
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
        assert!(feedback.description.contains("<text>"));
    }

    #[test]
    fn one_entry_owns_completion_and_action() {
        let new = builtin_command("new").unwrap();
        assert_eq!(new.completion, CompletionKind::NewMode);
        assert_eq!(new.action, CommandAction::New);
        assert_eq!(completion_context("/new m").unwrap().1, "m");
        assert_eq!(completion_context("/skill").unwrap().1, "");
    }

    #[test]
    fn fuzzy_order_is_prefix_then_substring_then_subsequence() {
        assert_eq!(match_command_catalog("set", &[])[0].line, "/settings");
        assert_eq!(match_command_catalog("ett", &[])[0].line, "/settings");
        assert_eq!(match_command_catalog("pln", &[])[0].line, "/plan");
    }
}
