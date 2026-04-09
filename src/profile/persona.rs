use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    /// Load a persona profile by name.
    ///
    /// Checks built-in profiles first, then loads from
    pub fn load_by_name(name: &str) -> Result<Self, PapertowelError> {
        if let Some(p) = Self::built_in_profiles()
            .into_iter()
            .find(|p| p.name == name)
        {
            return Ok(p);
        }
        let path = profiles_dir().join(format!("{name}.toml"));
        Self::load_from_file(&path)
    }
}

pub fn profiles_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").map_or_else(
        |_| {
            std::env::var("HOME").map_or_else(
                |_| PathBuf::from(".config"),
                |h| PathBuf::from(h).join(".config"),
            )
        },
        PathBuf::from,
    );
    base.join("papertowel").join("profiles")
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

    let valid = parts.first().is_some_and(|p| is_valid_hhmm(p))
        && parts.get(1).is_some_and(|p| is_valid_hhmm(p));
    if valid {
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

    let Some(hour) = parts.first().and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    let Some(minute) = parts.get(1).and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };

    hour < 24 && minute < 60
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic, reason = "test assertion helpers")]

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
    fn profile_roundtrips_toml() -> Result<(), Box<dyn std::error::Error>> {
        let profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .ok_or("no built-in profiles")?;

        let rendered = profile.to_toml_string()?;
        let parsed = PersonaProfile::from_toml_str(&rendered)?;
        assert_eq!(parsed, profile);
        Ok(())
    }

    #[test]
    fn invalid_probability_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let mut profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .ok_or("no built-in profiles")?;
        profile.messages.emoji_rate = 1.5;

        let validate = profile.validate();
        assert!(validate.is_err());
        Ok(())
    }

    #[test]
    fn save_and_load_profile_file() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let file_path = tmp.path().join("night-owl.toml");
        let profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .ok_or("no built-in profiles")?;

        profile.save_to_file(&file_path)?;
        assert!(file_path.exists());

        let loaded = PersonaProfile::load_from_file(&file_path)?;
        assert_eq!(loaded, profile);

        let raw = fs::read_to_string(&file_path)?;
        assert!(!raw.is_empty());
        Ok(())
    }

    #[test]
    fn validate_rejects_empty_name() {
        let mut profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no built-in profiles"));
        profile.name = String::new();
        assert!(
            profile.validate().is_err(),
            "empty name should fail validation"
        );
    }

    #[test]
    fn validate_rejects_empty_timezone() {
        let mut profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no built-in profiles"));
        profile.timezone = String::new();
        assert!(
            profile.validate().is_err(),
            "empty timezone should fail validation"
        );
    }

    #[test]
    fn validate_rejects_zero_commits_per_session() {
        let mut profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no built-in profiles"));
        profile.schedule.avg_commits_per_session = 0;
        assert!(
            profile.validate().is_err(),
            "zero commits per session should fail"
        );
    }

    #[test]
    fn validate_rejects_invalid_time_range_format() {
        let mut profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no built-in profiles"));
        profile.schedule.active_hours = vec![String::from("25:00-26:00")];
        assert!(
            profile.validate().is_err(),
            "invalid time range should fail"
        );
    }

    #[test]
    fn validate_rejects_active_hours_wrong_delimiter() {
        let mut profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no built-in profiles"));
        profile.schedule.active_hours = vec![String::from("09:00 17:00")];
        assert!(
            profile.validate().is_err(),
            "missing dash delimiter should fail"
        );
    }

    #[test]
    fn default_impls_are_sane() {
        use crate::profile::persona::{PersonaArchaeology, PersonaMessages, PersonaSchedule};
        let sched = PersonaSchedule::default();
        assert!(!sched.active_hours.is_empty());
        let msgs = PersonaMessages::default();
        assert!(msgs.wip_frequency > 0.0);
        let arch = PersonaArchaeology::default();
        assert!(arch.rename_chains);
    }

    #[test]
    fn validate_rejects_empty_active_hours() {
        // Covers line 203-205: empty active_hours → validation error.
        let mut profile = PersonaProfile::built_in_profiles()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no built-in profiles"));
        profile.schedule.active_hours = Vec::new();
        assert!(
            profile.validate().is_err(),
            "empty active_hours should fail validation"
        );
    }

    #[test]
    fn is_valid_hhmm_rejects_missing_colon() {
        // Covers line 249-250: parts.len()!= 2 → false.
        // A string without a colon → split by ':' gives 1 part.
        use super::is_valid_hhmm;
        assert!(!is_valid_hhmm("1200"), "no colon → invalid");
        assert!(!is_valid_hhmm(""), "empty → invalid");
    }

    #[test]
    fn is_valid_hhmm_rejects_non_numeric_parts() {
        // Covers lines 253-255 (hour parse fails) and 256-257 (minute parse fails).
        use super::is_valid_hhmm;
        assert!(!is_valid_hhmm("ab:30"), "non-numeric hour → invalid");
        assert!(!is_valid_hhmm("10:xy"), "non-numeric minute → invalid");
    }
}
