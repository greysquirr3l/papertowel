#[derive(Debug, Clone)]
pub struct PersonaProfile {
    pub name: String,
    pub timezone: String,
}

impl PersonaProfile {
    pub fn built_in_profiles() -> [Self; 2] {
        [
            Self {
                name: String::from("night-owl"),
                timezone: String::from("America/Detroit"),
            },
            Self {
                name: String::from("nine-to-five"),
                timezone: String::from("UTC"),
            },
        ]
    }
}