#[derive(Debug, Clone, Default)]
pub struct ScoreBreakdown {
    pub file_score: f32,
    pub repo_score: f32,
    pub history_score: f32,
}

impl ScoreBreakdown {
    pub fn total(&self) -> f32 {
        self.file_score + self.repo_score + self.history_score
    }
}