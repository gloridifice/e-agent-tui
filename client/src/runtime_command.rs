//! Central slash-command registry and dispatcher.
//!
//! Every optimized (built-in) command is declared exactly once in
//! [`BUILTIN_COMMANDS`]: the same entry owns its description, argument
//! completion policy, and runtime effect. DSH/plugin commands arrive as
//! [`CommandInfo`] descriptors and are merged into discovery automatically;
//! built-ins win name collisions while unknown names use the generic bridge
//! executor.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::{
    config::{Config, Theme},
    copy::CopyMode,
    input::InputState,
    input_page::InputPageSession,
    model::{AppState, Msg},
    protocol::{ClientMessage, CommandInfo},
    settings,
    theme::ThemeFile,
    ui::PickerState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    None,
    NewMode,
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandAction {
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
    Copy,
    Quit,
}

/// One optimized command. `name` never includes the leading slash.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub input_hint: Option<&'static str>,
    pub completion: CompletionKind,
    action: CommandAction,
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
    command!("copy", "进入复制模式", None, None, Copy),
    command!("clear", "清空会话列表", None, None, Forward),
    command!("exit", "退出客户端", None, None, Quit),
    command!("q", "退出客户端", None, None, Quit),
    command!("quit", "退出客户端", None, None, Quit),
];

pub fn builtin_command(name: &str) -> Option<&'static BuiltinCommand> {
    BUILTIN_COMMANDS.iter().find(|command| command.name == name)
}

/// Resolve an argument-completion context from the central declaration. This
/// deliberately returns no context for commands whose completion is `None`,
/// so input handling never needs a second list of command names.
pub fn completion_context(line: &str) -> Option<(&'static BuiltinCommand, &str)> {
    let body = line.strip_prefix('/')?;

    // `/skill` starts the roster immediately; both the canonical colon form
    // and the bridge-compatible space form continue filtering it.
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
    query.chars().all(|c| chars.any(|candidate| candidate == c))
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
/// result. A built-in shadows a same-name DSH descriptor, so one command is
/// never displayed or registered twice.
pub fn match_command_catalog(query: &str, integrated: &[CommandInfo]) -> Vec<CommandCandidate> {
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
                    command.input.as_ref().map(|input| input.hint.as_str()),
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

pub enum CommandOutcome {
    Continue,
    Quit,
}

pub struct LocalCommandContext<'a> {
    pub input_page: &'a mut Option<InputPageSession>,
    pub picker: &'a mut Option<PickerState>,
    pub help_visible: &'a mut bool,
    pub copy_mode: &'a mut Option<CopyMode>,
    pub config: &'a mut Config,
    pub themes: &'a mut Vec<ThemeFile>,
    pub input: &'a mut InputState,
    pub theme: &'a mut Theme,
    pub state: &'a Arc<Mutex<AppState>>,
    pub outbound: &'a mpsc::Sender<ClientMessage>,
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

/// Append a client-side usage error and invalidate the transcript cache.
/// Pushing to `msgs` without invalidating leaves the new line invisible until
/// an unrelated structural event forces a rebuild — the single source of truth
/// for "error message + re-render" keeps both rejection paths consistent.
fn push_error(state: &Arc<Mutex<AppState>>, text: impl Into<String>) {
    let mut state = state.lock().unwrap();
    state.msgs.push(Msg::Error { text: text.into() });
    state.transcript_cache.invalidate();
}

async fn forward(line: String, context: &LocalCommandContext<'_>) {
    let _ = context.outbound.send(ClientMessage::Command { line }).await;
}

pub async fn handle_local_command(
    line: String,
    context: LocalCommandContext<'_>,
) -> CommandOutcome {
    let Some((name, raw_input)) = parse_line(&line) else {
        return CommandOutcome::Continue;
    };
    let Some(command) = builtin_command(name) else {
        // Auto-discovered DSH/plugin command: generic command-plane adapter.
        forward(line, &context).await;
        return CommandOutcome::Continue;
    };

    match command.action {
        CommandAction::Settings => {
            if reject_arguments(&context, name, raw_input) {
                return CommandOutcome::Continue;
            }
            *context.input_page = Some(InputPageSession::settings(settings::SettingsState {
                modes: context
                    .input
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
                return CommandOutcome::Continue;
            }
            *context.input_page = Some(InputPageSession::login());
            let _ = context.outbound.send(ClientMessage::LoginGet).await;
        }
        CommandAction::Theme => {
            if reject_arguments(&context, name, raw_input) {
                return CommandOutcome::Continue;
            }
            *context.input_page = Some(InputPageSession::theme(
                context.themes,
                &context.config.theme,
            ));
        }
        CommandAction::Model => {
            if reject_arguments(&context, name, raw_input) {
                return CommandOutcome::Continue;
            }
            *context.input_page = Some(InputPageSession::model());
            let _ = context.outbound.send(ClientMessage::ModelGet).await;
        }
        CommandAction::Reload => {
            if reject_arguments(&context, name, raw_input) {
                return CommandOutcome::Continue;
            }
            *context.config = Config::load();
            *context.themes = crate::theme::load_themes(&Config::themes_dir());
            context.config.resolved_theme =
                crate::theme::resolve(&context.config.theme, context.themes);
            {
                let mut state = context.state.lock().unwrap();
                state.config = context.config.clone();
                state.transcript_cache.invalidate();
                state.msgs.push(Msg::System {
                    text: "已重载配置、主题与技能".into(),
                });
            }
            *context.theme = context.config.theme();
            context.input.paste_placeholder_chars = context.config.paste_placeholder_chars;
            context.input.history_limit = context.config.history_limit;
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
                return CommandOutcome::Quit;
            }
        }
        CommandAction::Resume => {
            let session_id = raw_input.trim();
            if session_id.is_empty() {
                *context.picker = Some(PickerState::default());
                let _ = context.outbound.send(ClientMessage::ListSessions).await;
            } else if session_id.split_whitespace().count() == 1 {
                let _ = context
                    .outbound
                    .send(ClientMessage::Attach {
                        session_id: session_id.to_owned(),
                    })
                    .await;
            } else {
                push_error(context.state, "用法: /resume [会话 ID]");
            }
        }
        // Bridge-optimized commands and known DSH commands still use the
        // command plane; their declaration remains local so richer argument
        // completion can be added without duplicating metadata elsewhere.
        CommandAction::New | CommandAction::Skill | CommandAction::Forward => {
            forward(line, &context).await
        }
    }
    CommandOutcome::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::CommandInputInfo;

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
