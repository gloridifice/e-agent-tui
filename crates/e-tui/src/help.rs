//! Provider-neutral help content for local frontend presentation.

use crate::{
    agent::CommandDescriptor,
    command_catalog::{match_command_catalog, CommandCandidate, CommandSource, CommandText},
    i18n::Language,
};

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn candidate_description(candidate: &CommandCandidate, language: Language) -> String {
    let (description, hint) = match &candidate.text {
        CommandText::Builtin {
            description_key,
            input_hint_key,
        } => (
            crate::i18n::tr(language, description_key),
            input_hint_key.map(|key| crate::i18n::tr(language, key)),
        ),
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
        let description = one_line(&candidate_description(command, language));
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
pub(crate) fn markdown(language: Language, integrated: &[CommandDescriptor]) -> String {
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
    let builtin_heading = crate::i18n::tr(language, "help.builtin_commands");
    let integrated_heading = crate::i18n::tr(language, "help.runtime_commands");
    append_commands(&mut output, &builtin_heading, &builtins, language);
    append_commands(&mut output, &integrated_heading, &integrated, language);
    output
}
