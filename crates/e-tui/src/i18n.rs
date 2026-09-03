//! Compile-time frontend localization with explicit per-call language.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

macro_rules! init_i18n {
    () => {
        rust_i18n::i18n!("locales", fallback = "en");
    };
}
pub(crate) use init_i18n;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::SimplifiedChinese];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Language {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "en" => Ok(Self::English),
            "zh-CN" => Ok(Self::SimplifiedChinese),
            _ => Err(format!("unsupported language: {value}")),
        }
    }
}

/// Resolve one frontend-owned key in an explicit language.
pub fn tr(language: Language, key: &str) -> String {
    rust_i18n::t!(key, locale = language.as_str()).into_owned()
}

/// Resolve one key and replace named `%{name}` arguments.
pub fn tr_args(language: Language, key: &str, arguments: &[(&str, String)]) -> String {
    let translated = tr(language, key);
    let patterns = arguments.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let values = arguments
        .iter()
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    rust_i18n::replace_patterns(&translated, &patterns, &values)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use super::*;

    #[test]
    fn language_parses_displays_and_round_trips_through_serde() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Document {
            language: Language,
        }

        for language in Language::ALL {
            assert_eq!(language.as_str().parse::<Language>(), Ok(language));
            assert_eq!(language.to_string(), language.as_str());
            let encoded = toml::to_string(&Document { language }).unwrap();
            assert_eq!(
                toml::from_str::<Document>(&encoded).unwrap(),
                Document { language }
            );
        }
        assert!("fr".parse::<Language>().is_err());
        assert!(toml::from_str::<Document>("language = \"fr\"").is_err());
    }

    #[test]
    fn lookup_uses_explicit_language_and_unknown_keys_fall_back_to_the_key() {
        assert_eq!(tr(Language::English, "common.on"), "On");
        assert_eq!(tr(Language::SimplifiedChinese, "common.on"), "开");
        assert_eq!(
            tr(Language::SimplifiedChinese, "missing.key"),
            "missing.key"
        );
        assert_eq!(
            tr_args(
                Language::English,
                "settings.item.default_mode.desc",
                &[("unused", "value".into())],
            ),
            "Mode used by bare /new and a fresh TUI; stale values fall back to standard"
        );
    }

    #[test]
    fn simplified_chinese_contains_every_english_key() {
        let backend = crate::_rust_i18n_backend();
        let keys = |locale| {
            backend
                .messages_for_locale(locale)
                .expect("locale is embedded")
                .into_iter()
                .map(|(key, _)| key.into_owned())
                .collect::<BTreeSet<_>>()
        };
        let english = keys("en");
        let chinese = keys("zh-CN");
        let missing = english.difference(&chinese).cloned().collect::<Vec<_>>();
        assert!(missing.is_empty(), "zh-CN is missing keys: {missing:?}");
    }

    fn assert_no_global_locale_mutation(path: &Path, needle: &str) {
        for entry in fs::read_dir(path).expect("source directory is readable") {
            let path = entry.expect("source entry is readable").path();
            if path.is_dir() {
                assert_no_global_locale_mutation(&path, needle);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = fs::read_to_string(&path).expect("Rust source is readable");
                assert!(
                    !source.contains(needle),
                    "global locale mutation is forbidden in {}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn production_source_does_not_mutate_the_global_locale() {
        let needle = ["set_", "locale("].concat();
        assert_no_global_locale_mutation(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &needle,
        );
    }
}
