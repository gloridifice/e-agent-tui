use super::*;
use crate::prompt_macros::{HeaderKind, PromptMacro, PromptMacros};

impl InputState {
    pub(crate) fn matched_macro<'a>(&self, macros: &'a PromptMacros) -> Option<&'a PromptMacro> {
        if self.search.is_some() {
            return None;
        }
        macros.exact_match(&self.buf, !self.image_blocks.is_empty())
    }

    pub(crate) fn catalog_changed_with_macros(
        &mut self,
        catalogs: &CatalogModel,
        macros: &PromptMacros,
    ) {
        if self
            .suggest
            .as_ref()
            .is_some_and(|s| s.kind == SuggestionKind::Files)
        {
            return;
        }
        let selected = self
            .suggest
            .as_ref()
            .and_then(|s| s.matches.get(s.sel))
            .cloned();
        self.suggest = None;
        self.refresh_suggest_with_macros(catalogs, macros);
        if let (Some(selected), Some(s)) = (selected, &mut self.suggest) {
            if let Some(index) = s.matches.iter().position(|item| item == &selected) {
                s.sel = index
            }
        }
    }

    pub(super) fn refresh_suggest_with_macros(
        &mut self,
        catalogs: &CatalogModel,
        macros: &PromptMacros,
    ) {
        let cursor = char_to_byte(&self.buf, self.cursor);
        let context = crate::prompt_macros::definition_context(&self.buf, cursor);
        self.macro_definition_completion = context.is_some();
        let (start, kind, ranked) = if let Some(context) = context {
            if context.kind == HeaderKind::Body {
                let start = self.buf[..context.range.start].chars().count();
                if self.block_ranges().any(|(block, _)| block < self.cursor)
                    || context.query.contains('\n')
                {
                    self.suggest = None;
                    self.command_completion = None;
                    return;
                }
                let active = self.command_completion.filter(|c| c.start == start);
                if active.is_some_and(|c| self.cursor > c.end) {
                    self.suggest = None;
                    return;
                }
                if active.is_some_and(|c| c.filled && self.cursor == c.end)
                    && self.suggest.is_some()
                {
                    return;
                }
                self.command_completion = Some(CommandCompletion {
                    start,
                    end: self.cursor,
                    query_cursor: self.cursor,
                    filled: false,
                });
                self.refresh_suggest(catalogs);
                return;
            }
            let query = context.query.to_lowercase();
            let matches: Vec<(String, String)> = match context.kind {
                HeaderKind::Id => macros
                    .entries()
                    .iter()
                    .filter(|entry| {
                        crate::command_catalog::rank_text(&query, &entry.id.to_lowercase())
                            .is_some()
                    })
                    .map(|entry| {
                        (
                            entry.id.clone(),
                            crate::prompt_macros::preview(&entry.prompt, 60),
                        )
                    })
                    .collect(),
                HeaderKind::Option => ["--model", "--effort", "--"]
                    .into_iter()
                    .filter(|option| {
                        (!context.has_model || *option != "--model" || context.query == "--model")
                            && (!context.has_effort
                                || *option != "--effort"
                                || context.query == "--effort")
                            && option.starts_with(&query)
                    })
                    .map(|option| (option.into(), String::new()))
                    .collect(),
                HeaderKind::Model => match_models(&query, &catalogs.model_providers)
                    .iter()
                    .map(|(provider, model)| {
                        (format!("{}/{}", provider.id, model.id), model.name.clone())
                    })
                    .collect(),
                HeaderKind::Effort => {
                    let efforts = if context.has_model {
                        context
                            .model
                            .and_then(|reference| {
                                crate::catalog::resolve_model_reference(
                                    &catalogs.model_providers,
                                    reference,
                                )
                            })
                            .and_then(|(_, model)| model.reasoning.as_ref())
                            .map(|r| r.efforts.as_slice())
                    } else {
                        catalogs.current_efforts()
                    };
                    match_efforts(&query, efforts.unwrap_or_default())
                        .iter()
                        .map(|effort| (effort.id.clone(), effort.name.clone()))
                        .collect()
                }
                HeaderKind::Body => unreachable!(),
            };
            (
                self.buf[..context.range.start].chars().count(),
                SuggestionKind::Subcommands,
                matches,
            )
        } else {
            let eligible = !self.buf.is_empty()
                && self.image_blocks.is_empty()
                && crate::prompt_macros::valid_id(self.buf.trim_end_matches(' '))
                && !self.buf[..cursor].contains(' ');
            if !eligible {
                self.refresh_suggest_at_cursor(catalogs);
                return;
            }
            let query = self.buf[..cursor].to_lowercase();
            let matches: Vec<(String, String)> = crate::command_catalog::rank_candidates(
                &query,
                macros.entries().iter(),
                |q, entry| crate::command_catalog::rank_text(q, &entry.id.to_lowercase()),
            )
            .iter()
            .map(|entry| {
                let mut description = crate::prompt_macros::preview(&entry.prompt, 60);
                if let Some(model) = &entry.model {
                    description.push_str(&format!(" · {model}"));
                }
                if let Some(effort) = &entry.effort {
                    description.push_str(&format!(" · {effort}"));
                }
                (entry.id.clone(), description)
            })
            .collect();
            (0, SuggestionKind::Macros, matches)
        };
        let active = self.command_completion.filter(|c| c.start == start);
        if active.is_some_and(|c| self.cursor > c.end) {
            self.suggest = None;
            return;
        }
        if active.is_some_and(|c| c.filled && c.end == self.cursor) && self.suggest.is_some() {
            return;
        }
        if self.block_ranges().any(|(block, _)| block < self.cursor) {
            self.suggest = None;
            self.command_completion = None;
            return;
        }
        self.command_completion = Some(CommandCompletion {
            start,
            end: self.cursor,
            query_cursor: self.cursor,
            filled: false,
        });
        let (matches, descriptions): (Vec<String>, Vec<String>) = ranked.into_iter().unzip();
        self.suggest = (!matches.is_empty()).then(|| Suggestion {
            query: self.command_prefix().into(),
            sel: 0,
            sources: vec![CommandSource::Builtin; matches.len()],
            matches,
            descriptions,
            kind,
        });
        if self.suggest.is_none() {
            self.command_completion = None
        }
    }
}
