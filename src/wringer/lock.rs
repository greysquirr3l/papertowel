use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use fs2::FileExt;

use crate::domain::errors::PapertowelError;

const LOCK_FILE_NAME: &str = "drip.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DripLockInfo {
    pub path: PathBuf,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub active: bool,
}

pub struct DripProcessLock {
    file: File,
}

impl DripProcessLock {
    pub fn acquire(repo_root: &Path) -> Result<Self, PapertowelError> {
        let state_dir = repo_root.join(".papertowel");
        fs::create_dir_all(&state_dir).map_err(|e| PapertowelError::io_with_path(&state_dir, e))?;

        let lock_path = lock_file_path(repo_root);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| PapertowelError::io_with_path(&lock_path, e))?;

        if let Err(error) = file.try_lock_exclusive() {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Err(PapertowelError::Config(format!(
                    "another papertowel drip instance is already running (lock: {})",
                    lock_path.display()
                )));
            }
            return Err(PapertowelError::io_with_path(&lock_path, error));
        }

        write_lock_metadata(&mut file, &lock_path)?;

        Ok(Self { file })
    }
}

impl Drop for DripProcessLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn read_lock_info(repo_root: &Path) -> Result<Option<DripLockInfo>, PapertowelError> {
    let lock_path = lock_file_path(repo_root);
    if !lock_path.exists() {
        return Ok(None);
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| PapertowelError::io_with_path(&lock_path, e))?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| PapertowelError::io_with_path(&lock_path, e))?;

    let mut pid: Option<u32> = None;
    let mut started_at: Option<String> = None;

    for line in contents.lines() {
        if let Some(raw_pid) = line.strip_prefix("pid=") {
            pid = raw_pid.trim().parse::<u32>().ok();
            continue;
        }
        if let Some(raw_started_at) = line.strip_prefix("started_at=") {
            let value = raw_started_at.trim();
            if !value.is_empty() {
                started_at = Some(value.to_owned());
            }
        }
    }

    let active = match file.try_lock_exclusive() {
        Ok(()) => {
            file.unlock()
                .map_err(|e| PapertowelError::io_with_path(&lock_path, e))?;
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
        Err(error) => return Err(PapertowelError::io_with_path(&lock_path, error)),
    };

    Ok(Some(DripLockInfo {
        path: lock_path,
        pid,
        started_at,
        active,
    }))
}

/// Remove `.papertowel/drip.lock` only when no process currently holds it.
///
/// Returns `true` when a stale file was removed.
pub fn recover_stale_lock(repo_root: &Path) -> Result<bool, PapertowelError> {
    let lock_path = lock_file_path(repo_root);
    if !lock_path.exists() {
        return Ok(false);
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| PapertowelError::io_with_path(&lock_path, e))?;

    match file.try_lock_exclusive() {
        Ok(()) => {
            file.unlock()
                .map_err(|e| PapertowelError::io_with_path(&lock_path, e))?;
            drop(file);
            fs::remove_file(&lock_path)
                .map_err(|e| PapertowelError::io_with_path(&lock_path, e))?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(PapertowelError::io_with_path(&lock_path, error)),
    }
}

fn lock_file_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".papertowel").join(LOCK_FILE_NAME)
}

fn write_lock_metadata(file: &mut File, lock_path: &Path) -> Result<(), PapertowelError> {
    let pid = std::process::id();
    let started_at = Utc::now().to_rfc3339();
    let payload = format!("pid={pid}\nstarted_at={started_at}\n");

    file.set_len(0)
        .map_err(|e| PapertowelError::io_with_path(lock_path, e))?;
    file.write_all(payload.as_bytes())
        .map_err(|e| PapertowelError::io_with_path(lock_path, e))?;
    file.sync_data()
        .map_err(|e| PapertowelError::io_with_path(lock_path, e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;

    use tempfile::TempDir;

    use crate::domain::errors::PapertowelError;

    use super::{DripProcessLock, read_lock_info, recover_stale_lock};

    #[test]
    fn second_lock_acquire_is_rejected() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;

        let first_lock = DripProcessLock::acquire(tmp.path())?;
        let second_lock = DripProcessLock::acquire(tmp.path());

        match second_lock {
            Err(PapertowelError::Config(message)) => {
                assert!(message.contains("already running"));
            }
            Err(other) => return Err(format!("unexpected error variant: {other}").into()),
            Ok(_) => return Err("second lock acquisition unexpectedly succeeded".into()),
        }

        drop(first_lock);

        let _third_lock = DripProcessLock::acquire(tmp.path())?;
        Ok(())
    }

    #[test]
    fn lock_file_is_created_with_metadata() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        let _lock = DripProcessLock::acquire(tmp.path())?;

        let lock_path = tmp.path().join(".papertowel").join("drip.lock");
        let contents = std::fs::read_to_string(&lock_path)?;

        assert!(contents.contains("pid="));
        assert!(contents.contains("started_at="));
        Ok(())
    }

    #[test]
    fn stale_lock_recovery_removes_unlocked_file() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        let state_dir = tmp.path().join(".papertowel");
        fs::create_dir_all(&state_dir)?;
        let lock_path = state_dir.join("drip.lock");
        fs::write(&lock_path, "pid=99999\nstarted_at=2026-04-09T00:00:00Z\n")?;

        let removed = recover_stale_lock(tmp.path())?;
        assert!(removed);
        assert!(!lock_path.exists());
        Ok(())
    }

    #[test]
    fn stale_lock_recovery_does_not_remove_active_lock() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        let _lock = DripProcessLock::acquire(tmp.path())?;

        let removed = recover_stale_lock(tmp.path())?;
        assert!(!removed);
        assert!(tmp.path().join(".papertowel").join("drip.lock").exists());
        Ok(())
    }

    #[test]
    fn read_lock_info_reports_active_state() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        let lock = DripProcessLock::acquire(tmp.path())?;

        let active = read_lock_info(tmp.path())?.ok_or("missing lock info")?;
        assert!(active.active);
        assert!(active.pid.is_some());
        assert!(active.started_at.is_some());

        drop(lock);

        let inactive = read_lock_info(tmp.path())?.ok_or("missing lock info")?;
        assert!(!inactive.active);
        Ok(())
    }
}
