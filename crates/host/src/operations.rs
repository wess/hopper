//! Conservative mutation admission: one writer per engine endpoint, across UI
//! and MCP processes. Reads remain concurrent; conflicting mutations fail
//! explicitly instead of queuing behind a potentially destructive old action.

use docker::{DockerError, Result};
use model::{OperationRecord, OperationState};
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn path(root: &Path, engine: &str) -> PathBuf {
    // Stable FNV hash is a filename, not authentication. A collision only
    // causes additional serialization; raw endpoints never enter the journal.
    let hash = engine.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    root.join("operations")
        .join(format!("engine{hash:016x}.json"))
}

pub fn recent(root: &Path, engine: &str) -> Result<Vec<OperationRecord>> {
    match store::json::read_bytes(&path(root, engine), 128 * 1024) {
        Ok(bytes) if bytes.len() <= 128 * 1024 => {
            serde_json::from_slice(&bytes).map_err(Into::into)
        }
        Ok(_) => Err(DockerError::decode(
            "Operation journal exceeds its size limit.",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

pub struct Guard {
    lock: File,
    path: PathBuf,
    records: Vec<OperationRecord>,
    finished: bool,
}

pub fn begin(root: &Path, engine: &str, action: &str) -> Result<Guard> {
    let path = path(root, engine);
    let directory = path.parent().unwrap();
    let mut directories = std::fs::DirBuilder::new();
    directories.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directories.mode(0o700);
    }
    directories.create(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(path.with_extension("lock"))?;
    lock.try_lock().map_err(|_| DockerError::api(409, "Another Hopper operation is changing this engine. Wait for it to finish before retrying."))?;
    let result = (|| {
        let mut records = recent(root, engine)?;
        // Acquiring the OS lock proves previous Running records have lost
        // their owner. Never replay them automatically after a crash.
        for record in &mut records {
            if record.state == OperationState::Running {
                record.state = OperationState::Interrupted;
                record.finished = Some(now());
            }
        }
        if records.len() >= 64 {
            records.drain(..records.len() - 63);
        }
        records.push(OperationRecord {
            id: model::new_uuid(),
            action: action.into(),
            state: OperationState::Running,
            started: now(),
            finished: None,
        });
        store::json::write(&path, &records)?;
        Ok(records)
    })();
    match result {
        Ok(records) => Ok(Guard {
            lock,
            path,
            records,
            finished: false,
        }),
        Err(error) => {
            let _ = lock.unlock();
            Err(error)
        }
    }
}

impl Guard {
    pub fn finish(mut self, succeeded: bool) {
        self.set(if succeeded {
            OperationState::Succeeded
        } else {
            OperationState::Failed
        });
        self.finished = true;
    }
    fn set(&mut self, state: OperationState) {
        if let Some(record) = self.records.last_mut() {
            record.state = state;
            record.finished = Some(now());
        }
        if let Err(error) = store::json::write(&self.path, &self.records) {
            tracing::error!("Could not persist operation outcome: {error}");
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            self.set(OperationState::Interrupted);
        }
        let _ = self.lock.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_excludes_other_writers_but_not_other_engines_and_records_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let guard = begin(root.path(), "first", "pull").unwrap();
        assert!(begin(root.path(), "first", "remove")
            .err()
            .unwrap()
            .is_conflict());
        begin(root.path(), "second", "start").unwrap().finish(true);
        drop(guard);
        let records = recent(root.path(), "first").unwrap();
        assert_eq!(records[0].state, OperationState::Interrupted);
        begin(root.path(), "first", "start").unwrap().finish(false);
        assert_eq!(
            recent(root.path(), "first").unwrap()[1].state,
            OperationState::Failed
        );
    }
    #[test]
    fn orphaned_records_are_marked_interrupted_and_history_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        for _ in 0..66 {
            begin(root.path(), "engine", "start").unwrap().finish(true);
        }
        let mut records = recent(root.path(), "engine").unwrap();
        assert_eq!(records.len(), 64);
        records.last_mut().unwrap().state = OperationState::Running;
        store::json::write(&path(root.path(), "engine"), &records).unwrap();
        begin(root.path(), "engine", "next").unwrap().finish(true);
        assert_eq!(
            recent(root.path(), "engine").unwrap()[62].state,
            OperationState::Interrupted
        );
    }
}
