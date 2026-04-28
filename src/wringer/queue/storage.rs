use std::fs;
use std::path::Path;

use crate::domain::errors::PapertowelError;

use super::QueuePlan;

pub(super) fn save_queue_plan(
    repo_path: impl AsRef<Path>,
    plan: &QueuePlan,
) -> Result<(), PapertowelError> {
    let state_dir = repo_path.as_ref().join(".papertowel");
    fs::create_dir_all(&state_dir).map_err(|e| PapertowelError::io_with_path(&state_dir, e))?;

    let path = state_dir.join("queue.json");
    let json = serde_json::to_string_pretty(plan)?;
    fs::write(&path, json).map_err(|e| PapertowelError::io_with_path(&path, e))?;
    Ok(())
}

pub(super) fn load_queue_plan(repo_path: impl AsRef<Path>) -> Result<QueuePlan, PapertowelError> {
    let path = repo_path.as_ref().join(".papertowel").join("queue.json");
    let json = fs::read_to_string(&path).map_err(|e| PapertowelError::io_with_path(&path, e))?;
    let plan = serde_json::from_str(&json)?;
    Ok(plan)
}
