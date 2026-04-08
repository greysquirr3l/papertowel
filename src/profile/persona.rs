use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::errors::PapertowelError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonaProfile {
    pub name: String,
    pub timezone: String,
    #[serde(default)]
    pub schedule: PersonaSchedule,
    #[serde(default)]
    pub messages: PersonaMessages,
    #[serde(default)]
    pub archaeology: PersonaArchaeology,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonaSchedule {
    pub active_hours: Vec<String>,
    pub peak_productivity: String,
    pub avg_commits_per_session: u16,
    pub session_variance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonaMessages {
    pub style: CommitMessageStyle,
    pub wip_frequency: f32,
    pub profanity_frequency: f32,
    pub typo_rate: f32,
    pub emoji_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CommitMessageStyle {
    Conventional,
    Lazy,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonaArchaeology {
    pub todo_inject_rate: f32,
    pub dead_code_rate: f32,
    pub rename_chains: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PersonaProfileDocument {
    persona: PersonaProfile,
}

impl Default for PersonaSchedule {
    fn default() -> Self {
        Self {
            active_hours: vec![String::from("09:00-12:00"), String::from("13:00-17:00")],
            peak_productivity: String::from("10:00-11:30"),
            avg_commits_per_session: 5,
            session_variance: 0.25,
        }
    }
}

impl Default for PersonaMessages {
    fn default() -> Self {
        Self {
            style: CommitMessageStyle::Mixed,
            wip_frequency: 0.10,
            profanity_frequency: 0.01,
            typo_rate: 0.02,
            emoji_rate: 0.00,
        }
    }
}

impl Default for PersonaArchaeology {
    fn default() -> Self {
        Self {
            todo_inject_rate: 0.10,
            dead_code_rate: 0.04,
            rename_chains: true,
        }
    }
}

impl PersonaProfile {
    pub fn built_in_profiles() -> [Self; 2] {
        [
            Self {
                name: String::from("night-owl"),
                timezone: String::from("America/Detroit"),
                schedule: PersonaSchedule {
                    active_hours: vec![String::from("10:00-14:00"), String::from("21:00-03:00")],
                    peak_productivity: String::from("22:00-01:00"),
                    avg_commits_per_session: 8,
                    session_variance: 0.40,
                },
                messages: PersonaMessages {
                    style: CommitMessageStyle::Mixed,
                    wip_frequency: 0.15,
                    profanity_frequency: 0.05,
                    typo_rate: 0.02,
                    emoji_rate: 0.01,
                },
                archaeology: PersonaArchaeology {
                    todo_inject_rate: 0.10,
                    dead_code_rate: 0.05,
                    rename_chains: true,
                },
            },
            Self {
                name: String::from("nine-to-five"),
                timezone: String::from("UTC"),
                schedule: PersonaSchedule {
                    active_hours: vec![String::from("09:00-12:00"), String::from("13:00-17:30")],
                    peak_productivity: String::from("10:00-11:30"),
                    avg_commits_per_session: 4,
                    session_variance: 0.20,
                },
                messages: PersonaMessages {
                    style: CommitMessageStyle::Conventional,
                    wip_frequency: 0.05,
                    profanity_frequency: 0.00,
                    typo_rate: 0.005,
                    emoji_rate: 0.00,
                },
                archaeology: PersonaArchaeology {
                    todo_inject_rate: 0.05,
                    dead_code_rate: 0.02,
                    rename_chains: false,
                },
            },
        ]
    }

    pub fn from_toml_str(content: &str) -> Result<Self, PapertowelError> {
        let doc: PersonaProfileDocument = toml::from_str(content)?;
        doc.persona.validate()?;
        Ok(doc.persona)
    }

    pub fn to_toml_string(&self) -> Result<String, PapertowelError> {
        self.validate()?;
        toml::to_string_pretty(&PersonaProfileDocument {
            persona: self.clone(),
        })
        .map_err(Into::into)
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, PapertowelError> {
        let path = path.as_ref();
        let raw =
            fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
        Self::from_toml_str(&raw)
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), PapertowelError> {
        let path = path.as_ref();
        let rendered = self.to_toml_string()?;
        fs::write(path, rendered).map_err(|error| PapertowelError::io_with_path(path, error))
    }

    pub fn validate(&self) -> Result<(), PapertowelError> {
        if self.name.trim().is_empty() {
            return Err(PapertowelError::Validation(
                "persona profile name cannot be empty".to_owned(),
            ));
        }

        if self.timezone.trim().is_empty() {
            return Err(PapertowelError::Validation(
                "persona timezone cannot be empty".to_owned(),
            ));
        }

        validate_probability("schedule.session_variance", self.schedule.session_variance)?;
        validate_probability("messages.wip_frequency", self.messages.wip_frequency)?;
        validate_probability(
            "messages.profanity_frequency",
            self.messages.profanity_frequency,
        )?;
        validate_probability("messages.typo_rate", self.messages.typo_rate)?;
        validate_probability("messages.emoji_rate", self.messages.emoji_rate)?;
        validate_probability(
            "archaeology.todo_inject_rate",
            self.archaeology.todo_inject_rate,
        )?;
        validate_probability(
            "archaeology.dead_code_rate",
            self.archaeology.dead_code_rate,
        )?;

        if self.schedule.avg_commits_per_session == 0 {
            return Err(PapertowelError::Validation(
                "schedule.avg_commits_per_session must be >= 1".to_owned(),
            ));
        }

        if self.schedule.active_hours.is_empty() {
            return Err(PapertowelError::Validation(
                "schedule.active_hours cannot be empty".to_owned(),
            ));
        }

        for range in &self.schedule.active_hours {
            validate_time_range(range)?;
        }
        validate_time_range(&self.schedule.peak_productivity)?;

        Ok(())
    }
}

fn validate_probability(field: &str, value: f32) -> Result<(), PapertowelError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(PapertowelError::Validation(format!(
            "{field} must be a finite value in [0.0, 1.0]"
        )))
    }
}

fn validate_time_range(value: &str) -> Result<(), PapertowelError> {
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(PapertowelError::Validation(format!(
            "invalid time range '{value}', expected HH:MM-HH:MM"
        )));
    }

    if is_valid_hhmm(parts[0]) && is_valid_hhmm(parts[1]) {
        Ok(())
    } else {
        Err(PapertowelError::Validation(format!(
            "invalid time range '{value}', expected HH:MM-HH:MM"
        )))
    }
}

fn is_valid_hhmm(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 {
        return false;
    }

    let hour = match parts.first().and_then(|part| part.parse::<u8>().ok()) {
        Some(hour) => hour,
        None => return false,
    };
    let minute = match parts.get(1).and_then(|part| part.parse::<u8>().ok()) {
        Some(minute) => minute,
        None => return false,
    };

    hour < 24 && minute < 60
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::profile::persona::PersonaProfile;

    #[test]
    fn built_in_profiles_are_valid() {
        for profile in PersonaProfile::built_in_profiles() {
            assert!(profile.validate().is_ok());
        }
    }

    #[test]
    fn profile_roundtrips_toml() {
        let profile = match PersonaProfile::built_in_profiles().first() {
            Some(profile) => profile.clone(),
            None => panic!("expected built-in profile"),
        };

        let rendered = profile.to_toml_string();
        assert!(rendered.is_ok());
        let rendered = match rendered {
            Ok(rendered) => rendered,
            Err(error) => panic!("unexpected render error: {error}"),
        };

        let parsed = PersonaProfile::from_toml_str(&rendered);
        assert!(parsed.is_ok());
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => panic!("unexpected parse error: {error}"),
        };

        assert_eq!(parsed, profile);
    }

    #[test]
    fn invalid_probability_rejected() {
        let mut profile = match PersonaProfile::built_in_profiles().first() {
            Some(profile) => profile.clone(),
            None => panic!("expected built-in profile"),
        };
        profile.messages.emoji_rate = 1.5;

        let validate = profile.validate();
        assert!(validate.is_err());
    }

    #[test]
    fn save_and_load_profile_file() {
        let tmp = TempDir::new();
        assert!(tmp.is_ok());
        let tmp = match tmp {
            Ok(tmp) => tmp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };

        let file_path = tmp.path().join("night-owl.toml");
        let profile = match PersonaProfile::built_in_profiles().first() {
            Some(profile) => profile.clone(),
            None => panic!("expected built-in profile"),
        };

        let save = profile.save_to_file(&file_path);
        assert!(save.is_ok());
        assert!(file_path.exists());

        let loaded = PersonaProfile::load_from_file(&file_path);
        assert!(loaded.is_ok());
        let loaded = match loaded {
            Ok(loaded) => loaded,
            Err(error) => panic!("unexpected load error: {error}"),
        };
        assert_eq!(loaded, profile);

        let raw = fs::read_to_string(&file_path);
        assert!(raw.is_ok());
    }
}
