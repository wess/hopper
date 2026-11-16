//! Cooperating-process transactions. Atomic replacement alone cannot prevent
//! lost updates between the app and its MCP sidecar.

use serde::{de::DeserializeOwned, Serialize};
use std::{collections::BTreeSet, fs::File, io, path::Path};

struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        // Explicit unlock also releases a lock if another thread forked a
        // short-lived helper while this descriptor was open.
        let _ = self.0.unlock();
    }
}

pub fn update<T, F>(path: &Path, change: F) -> io::Result<T>
where
    T: DeserializeOwned + Serialize + Default,
    F: FnOnce(T) -> io::Result<T>,
{
    if let Some(parent) = path.parent() {
        let mut directories = std::fs::DirBuilder::new();
        directories.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directories.mode(0o700);
        }
        directories.create(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = Lock(options.open(path.with_extension("lock"))?);
    lock.0.try_lock().map_err(|_| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "Another Hopper process is saving this document. Try again.",
        )
    })?;
    let current = match super::json::read_bytes(path, 8 * 1024 * 1024) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => T::default(),
        Err(e) => return Err(e),
    };
    let next = change(current)?;
    super::json::write(path, &next)?;
    Ok(next)
}

/// Merge fields changed from a known base while preserving changes made by a
/// second process. Arrays and nested objects are indivisible fields: e.g. two
/// concurrent edits of folder privileges must be reviewed, never combined.
pub fn merge<T: Serialize + DeserializeOwned>(base: &T, next: &T, current: T) -> io::Result<T> {
    let base = serde_json::to_value(base).map_err(io::Error::other)?;
    let next = serde_json::to_value(next).map_err(io::Error::other)?;
    let mut current = serde_json::to_value(current).map_err(io::Error::other)?;
    let object = current
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Expected a settings object."))?;
    let keys: BTreeSet<_> = base
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .chain(next.as_object().into_iter().flat_map(|o| o.keys()))
        .collect();
    for key in keys {
        if base.get(key) == next.get(key) {
            continue;
        }
        if object.get(key) != base.get(key) && object.get(key) != next.get(key) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Settings field {key} changed in another process. Reload before saving."),
            ));
        }
        match next.get(key) {
            Some(value) => {
                object.insert(key.clone(), value.clone());
            }
            None => {
                object.remove(key);
            }
        }
    }
    serde_json::from_value(current).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Settings, ThemeMode};

    #[test]
    fn disjoint_edits_merge_and_same_field_conflicts_preserve_disk() {
        let base = Settings::default();
        let mut first = base.clone();
        first.theme = ThemeMode::Dark;
        let mut second = base.clone();
        second.autostart_engine = false;
        let merged = merge(&base, &second, first.clone()).unwrap();
        assert_eq!(merged.theme, ThemeMode::Dark);
        assert!(!merged.autostart_engine);
        second.theme = ThemeMode::Light;
        assert_eq!(
            merge(&base, &second, first).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn corrupt_documents_are_not_replaced_by_an_update() {
        let path =
            std::env::temp_dir().join(format!("hoppertransaction{}.json", std::process::id()));
        std::fs::write(&path, b"broken").unwrap();
        assert!(update::<Settings, _>(&path, Ok).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(path.with_extension("lock")).unwrap();
    }
}
