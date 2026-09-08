//! Persisted letter associations use exact provider/model identities.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMark {
    pub letter: char,
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<ModelMark>", into = "Vec<ModelMark>")]
pub struct ModelMarks(Vec<ModelMark>);

impl TryFrom<Vec<ModelMark>> for ModelMarks {
    type Error = String;

    fn try_from(marks: Vec<ModelMark>) -> Result<Self, Self::Error> {
        for (index, mark) in marks.iter().enumerate() {
            if !mark.letter.is_ascii_lowercase() {
                return Err("model mark must be a lowercase ASCII letter".into());
            }
            if marks[..index].iter().any(|other| {
                other.letter == mark.letter
                    || (other.provider == mark.provider && other.model == mark.model)
            }) {
                return Err("duplicate model mark letter or route".into());
            }
        }
        Ok(Self(marks))
    }
}

impl From<ModelMarks> for Vec<ModelMark> {
    fn from(marks: ModelMarks) -> Self {
        marks.0
    }
}

/// Return the mark and the byte offset of the prompt body.
pub fn prefix(text: &str) -> Option<(char, usize)> {
    let rest = text.strip_prefix("//")?;
    let letter = rest.chars().next()?;
    if !letter.is_ascii_lowercase() {
        return None;
    }
    let tail = &rest[1..];
    if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
        return None;
    }
    Some((letter, text.len() - tail.trim_start().len()))
}

impl ModelMarks {
    pub fn get(&self, letter: char) -> Option<&ModelMark> {
        self.0.iter().find(|mark| mark.letter == letter)
    }

    pub fn letter(&self, provider: &str, model: &str) -> Option<char> {
        self.0
            .iter()
            .find(|mark| mark.provider == provider && mark.model == model)
            .map(|mark| mark.letter)
    }

    pub fn toggle(&mut self, letter: char, provider: &str, model: &str) {
        if !letter.is_ascii_lowercase() {
            return;
        }
        let remove = self.letter(provider, model) == Some(letter);
        self.0.retain(|mark| {
            mark.letter != letter && (mark.provider != provider || mark.model != model)
        });
        if !remove {
            self.0.push(ModelMark {
                letter,
                provider: provider.into(),
                model: model.into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn model_marks_toggle_reassign_and_round_trip() {
        let mut config = Config::from_user_toml("theme = 'ferra'").unwrap();
        assert_eq!(config.model_marks.get('a'), None);
        config.model_marks.toggle('a', "p", "m");
        config.model_marks.toggle('b', "p", "other");
        config.model_marks.toggle('b', "p", "m");
        assert_eq!(config.model_marks.get('a'), None);
        assert_eq!(config.model_marks.letter("p", "other"), None);
        assert_eq!(config.model_marks.letter("p", "m"), Some('b'));
        config.model_marks.toggle('a', "other-provider", "m");
        let serialized = toml::to_string(&config).unwrap();
        let mut restored = Config::from_user_toml(&serialized).unwrap();
        assert_eq!(restored.model_marks, config.model_marks);
        restored.model_marks.toggle('b', "p", "m");
        assert_eq!(restored.model_marks.get('b'), None);
        assert_eq!(
            restored.model_marks.letter("other-provider", "m"),
            Some('a')
        );
    }

    #[test]
    fn model_marks_reject_invalid_persisted_associations() {
        for source in [
            "model_marks = [{ letter = 'A', provider = 'p', model = 'm' }]",
            "model_marks = [{ letter = '1', provider = 'p', model = 'm' }]",
            "model_marks = [{ letter = 'a', provider = 'p', model = 'm', unknown = true }]",
            "model_marks = [{ letter = 'a', provider = 'p', model = 'm' }, { letter = 'a', provider = 'q', model = 'm' }]",
            "model_marks = [{ letter = 'a', provider = 'p', model = 'm' }, { letter = 'b', provider = 'p', model = 'm' }]",
        ] {
            assert!(Config::from_user_toml(source).is_err(), "{source}");
        }
    }
}
