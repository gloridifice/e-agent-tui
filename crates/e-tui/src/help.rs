//! Provider-neutral help content for local frontend presentation.

use crate::{
    agent::CommandDescriptor,
    command_catalog::{
        candidate_description, match_command_catalog, CommandCandidate, CommandSource,
    },
    i18n::Language,
};

pub(crate) fn action_label(language: Language, action: crate::key_mapping::Action) -> String {
    crate::i18n::tr(language, &format!("key.action.{}", action.name()))
}

pub(crate) fn key_hints(
    config: &crate::Config,
    scope: crate::key_mapping::Scope,
    actions: &[crate::key_mapping::Action],
) -> String {
    actions
        .iter()
        .map(|&action| {
            format!(
                "{} {}",
                config.key_mapping.label(scope, action),
                action_label(config.language, action)
            )
        })
        .collect::<Vec<_>>()
        .join("   ")
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn append_commands(
    output: &mut String,
    heading: &str,
    commands: &[&CommandCandidate],
    language: Language,
) {
    if commands.is_empty() {
        return;
    }
    output.push_str("\n## ");
    output.push_str(heading);
    output.push_str("\n\n");
    for command in commands {
        let line = command.line.replace('`', "");
        let description = one_line(&candidate_description(command, |key| {
            crate::i18n::tr(language, key)
        }));
        output.push_str("- `");
        output.push_str(&line);
        output.push('`');
        if !description.is_empty() {
            output.push_str(if language == Language::SimplifiedChinese {
                "："
            } else {
                ": "
            });
            output.push_str(&description);
        }
        output.push('\n');
    }
}

/// Build the complete local `/help` Markdown from authoritative catalogs.
pub(crate) fn markdown(config: &crate::Config, integrated: &[CommandDescriptor]) -> String {
    let language = config.language;
    let commands = match_command_catalog("", integrated);
    let builtins = commands
        .iter()
        .filter(|command| command.source == CommandSource::Builtin)
        .collect::<Vec<_>>();
    let integrated = commands
        .iter()
        .filter(|command| command.source == CommandSource::Integrated)
        .collect::<Vec<_>>();

    let mut output = crate::i18n::tr(language, "help.body").trim_end().to_owned();
    output.push('\n');
    for scope in crate::key_mapping::Scope::ALL {
        output.push_str(&format!("\n### {}\n\n", scope.name()));
        for (_, action) in config.key_mapping.entries().filter(|(s, _)| *s == scope) {
            output.push_str(&format!(
                "- `{}`: {} (`{}`)\n",
                config.key_mapping.label(scope, action),
                action_label(language, action),
                action.name()
            ));
        }
    }
    let builtin_heading = crate::i18n::tr(language, "help.builtin_commands");
    let integrated_heading = crate::i18n::tr(language, "help.runtime_commands");
    append_commands(&mut output, &builtin_heading, &builtins, language);
    append_commands(&mut output, &integrated_heading, &integrated, language);
    output
}
