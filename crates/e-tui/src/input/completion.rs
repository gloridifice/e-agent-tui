use crate::agent::{ModelDescriptor, ModelProvider, ReasoningEffort, Skill};
use crate::command_catalog::{rank_candidates, rank_fields, NewMode};

/// Ranked fuzzy match against the `/new` modes: id-prefix matches first,
/// then id/display-name substring matches, then subsequence matches; each
/// group keeps the roster order. An empty query returns every mode.
pub fn match_new_modes<'a>(query: &str, modes: &'a [NewMode]) -> Vec<&'a NewMode> {
    let query = query.to_lowercase();
    rank_candidates(&query, modes.iter(), |query, mode| {
        rank_fields(
            query,
            [
                mode.id.to_lowercase(),
                mode.name.as_deref().unwrap_or_default().to_lowercase(),
            ],
        )
    })
}

/// Fuzzy-rank model routes using model/provider IDs, canonical routes, and
/// display names. The result keeps provider roster order inside each rank.
pub fn match_models<'a>(
    query: &str,
    providers: &'a [ModelProvider],
) -> Vec<(&'a ModelProvider, &'a ModelDescriptor)> {
    let query = query.to_lowercase();
    let items = providers
        .iter()
        .flat_map(|provider| provider.models.iter().map(move |model| (provider, model)));
    rank_candidates(&query, items, |query, (provider, model)| {
        rank_fields(
            query,
            [
                model.id.to_lowercase(),
                provider.id.to_lowercase(),
                format!("{}/{}", provider.id, model.id).to_lowercase(),
                model.name.to_lowercase(),
            ],
        )
    })
}

pub fn match_efforts<'a>(query: &str, efforts: &'a [ReasoningEffort]) -> Vec<&'a ReasoningEffort> {
    let query = query.to_lowercase();
    rank_candidates(&query, efforts.iter(), |query, effort| {
        rank_fields(
            query,
            [effort.id.to_lowercase(), effort.name.to_lowercase()],
        )
    })
}

pub fn match_skills<'a>(query: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    let query = query.to_lowercase();
    rank_candidates(&query, skills.iter(), |query, skill| {
        rank_fields(query, [skill.name.to_lowercase()])
    })
}
