//! Frontend-only effort preferences keyed by exact provider/model identities.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefaultEffort {
    pub provider: String,
    pub model: String,
    pub effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<ModelDefaultEffort>", into = "Vec<ModelDefaultEffort>")]
pub struct ModelDefaultEfforts(Vec<ModelDefaultEffort>);

impl TryFrom<Vec<ModelDefaultEffort>> for ModelDefaultEfforts {
    type Error = String;

    fn try_from(entries: Vec<ModelDefaultEffort>) -> Result<Self, Self::Error> {
        for (index, entry) in entries.iter().enumerate() {
            if [&entry.provider, &entry.model, &entry.effort]
                .iter()
                .any(|value| value.trim().is_empty())
            {
                return Err(
                    "model default effort requires nonempty provider, model and effort".into(),
                );
            }
            if entries[..index]
                .iter()
                .any(|other| other.provider == entry.provider && other.model == entry.model)
            {
                return Err("duplicate model default effort route".into());
            }
        }
        Ok(Self(entries))
    }
}

impl From<ModelDefaultEfforts> for Vec<ModelDefaultEffort> {
    fn from(defaults: ModelDefaultEfforts) -> Self {
        defaults.0
    }
}

impl ModelDefaultEfforts {
    pub fn get(&self, provider: &str, model: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| entry.provider == provider && entry.model == model)
            .map(|entry| entry.effort.as_str())
    }

    pub fn set(&mut self, provider: &str, model: &str, effort: &str) {
        if let Some(entry) = self
            .0
            .iter_mut()
            .find(|entry| entry.provider == provider && entry.model == model)
        {
            entry.effort = effort.into();
        } else {
            self.0.push(ModelDefaultEffort {
                provider: provider.into(),
                model: model.into(),
                effort: effort.into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn model_defaults_round_trip_update_and_keep_providers_separate() {
        let mut config = Config::from_user_toml("theme = 'ferra'").unwrap();
        assert_eq!(config.model_default_efforts.get("p", "m"), None);
        config.model_default_efforts.set("p", "m", "low");
        config.model_default_efforts.set("q", "m", "medium");
        config.model_default_efforts.set("p", "m", "high");
        let text = toml::to_string_pretty(&config).unwrap();
        let restored = Config::from_user_toml(&text).unwrap();
        assert_eq!(restored.model_default_efforts, config.model_default_efforts);
        assert_eq!(restored.model_default_efforts.get("p", "m"), Some("high"));
        assert_eq!(restored.model_default_efforts.get("q", "m"), Some("medium"));
        assert_eq!(restored.model_default_efforts.get("P", "m"), None);
    }

    #[test]
    fn model_defaults_reject_malformed_and_duplicate_routes() {
        for entries in [
            "[{ provider = 'p', model = 'm' }]",
            "[{ provider = '', model = 'm', effort = 'high' }]",
            "[{ provider = 'p', model = ' ', effort = 'high' }]",
            "[{ provider = 'p', model = 'm', effort = '' }]",
            "[{ provider = 'p', model = 'm', effort = 1 }]",
            "[{ provider = 'p', model = 'm', effort = 'high', unknown = true }]",
            "[{ provider = 'p', model = 'm', effort = 'high' }, { provider = 'p', model = 'm', effort = 'low' }]",
        ] {
            assert!(Config::from_user_toml(&format!("model_default_efforts = {entries}")).is_err());
        }
    }
}
