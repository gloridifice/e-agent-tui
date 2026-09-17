//! Provider-neutral help content for local frontend presentation.

use crate::{
    i18n::{tr, tr_args, Language},
    key_mapping::{Action, Scope},
};

pub(crate) fn action_label(language: Language, action: Action) -> String {
    tr(language, &format!("key.action.{}", action.name()))
}

pub(crate) fn key_hints(config: &crate::Config, scope: Scope, actions: &[Action]) -> String {
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

fn table_cell(value: &str) -> String {
    one_line(value).replace('|', "\\|")
}

fn code_cell(value: &str) -> String {
    let value = table_cell(value);
    if value.contains('`') {
        format!("`` {value} ``")
    } else {
        format!("`{value}`")
    }
}

fn scope_actions(config: &crate::Config, scope: Scope) -> Vec<Action> {
    config
        .key_mapping
        .entries()
        .filter_map(|(candidate, action)| (candidate == scope).then_some(action))
        .collect()
}

fn append_group(
    output: &mut String,
    config: &crate::Config,
    heading_key: &str,
    scopes: &[(Scope, Option<&str>)],
) {
    let populated = scopes
        .iter()
        .filter_map(|&(scope, context_key)| {
            let actions = scope_actions(config, scope);
            (!actions.is_empty()).then_some((scope, context_key, actions))
        })
        .collect::<Vec<_>>();
    if populated.is_empty() {
        return;
    }

    let language = config.language;
    output.push_str("\n\n## ");
    output.push_str(&tr(language, heading_key));
    output.push('\n');
    for (scope, context_key, actions) in populated {
        if let Some(context_key) = context_key {
            output.push_str("\n### ");
            output.push_str(&tr(language, context_key));
            output.push('\n');
        }
        output.push_str("\n| ");
        output.push_str(&tr(language, "help.table.key"));
        output.push_str(" | ");
        output.push_str(&tr(language, "help.table.action"));
        output.push_str(" |\n| --- | --- |\n");
        for action in actions {
            output.push_str("| ");
            output.push_str(&code_cell(&config.key_mapping.label(scope, action)));
            output.push_str(" | ");
            output.push_str(&table_cell(&action_label(language, action)));
            output.push_str(" |\n");
        }
    }
}

/// Build the complete local help Markdown from the effective key mapping.
pub(crate) fn markdown(config: &crate::Config) -> String {
    let mut output = tr(config.language, "help.body").trim_end().to_owned();

    append_group(
        &mut output,
        config,
        "help.section.global",
        &[(Scope::Global, None)],
    );
    append_group(
        &mut output,
        config,
        "help.section.message",
        &[
            (Scope::Message, Some("help.context.common")),
            (Scope::MessageIdle, Some("help.context.idle")),
            (Scope::MessageWorking, Some("help.context.working")),
            (Scope::MessageEdit, Some("help.context.editing")),
        ],
    );
    append_group(
        &mut output,
        config,
        "help.section.completion",
        &[
            (Scope::MessageSuggest, Some("help.context.suggestions")),
            (Scope::MessageSearch, Some("help.context.search")),
        ],
    );
    append_group(
        &mut output,
        config,
        "help.section.reading",
        &[
            (Scope::ReadMode, Some("help.context.blocks")),
            (Scope::ReadModeItem, Some("help.context.items")),
            (Scope::FullScreen, Some("help.context.full_screen")),
            (Scope::History, Some("help.context.history")),
        ],
    );
    append_group(
        &mut output,
        config,
        "help.section.pages",
        &[
            (Scope::Page, Some("help.context.page")),
            (Scope::PageEdit, Some("help.context.page_edit")),
            (Scope::PageChoice, Some("help.context.page_choice")),
            (Scope::PageResume, Some("help.context.resume")),
            (Scope::PageQuestion, Some("help.context.question")),
            (Scope::PageQuestionEdit, Some("help.context.question_edit")),
            (Scope::Approval, Some("help.context.approval")),
        ],
    );
    append_group(
        &mut output,
        config,
        "help.section.help",
        &[(Scope::Help, None)],
    );

    output.push_str("\n\n## ");
    output.push_str(&tr(config.language, "help.section.tips"));
    output.push_str("\n\n");
    for key in [
        "help.command_completion",
        "help.path_completion",
        "help.model_marks",
        "help.model_prefix",
        "help.link_copy",
        "help.mouse_copy",
        "help.mouse_resize",
        "help.config",
    ] {
        output.push_str("- ");
        output.push_str(&tr(config.language, key));
        output.push('\n');
    }
    output
}

pub(crate) fn footer(config: &crate::Config) -> String {
    let mapping = &config.key_mapping;
    tr_args(
        config.language,
        "help.footer",
        &[
            (
                "lines",
                format!(
                    "{} / {}",
                    mapping.label(Scope::Help, Action::MoveUp),
                    mapping.label(Scope::Help, Action::MoveDown)
                ),
            ),
            (
                "pages",
                format!(
                    "{} / {}",
                    mapping.label(Scope::Help, Action::MoveUpFast),
                    mapping.label(Scope::Help, Action::MoveDownFast)
                ),
            ),
            ("close", mapping.label(Scope::Help, Action::Close)),
        ],
    )
}
