//! Reading and writing the JSON documents under `~/.hopper/`.
//!
//! Writes go to a temporary file in the same directory and are renamed into
//! place, so a crash mid-write leaves the previous document intact rather than
//! a truncated one. A settings file the user cannot load is a support ticket;
//! a settings file that lost the last change is a shrug.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;

    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();

    // Windows refuses a replace while another writer's replace of the same
    // document is in flight (access denied), or while something holds it open
    // (sharing violation). Both clear in milliseconds, so wait them out rather
    // than report a failed save for two settings writes that overlapped.
    let mut attempt: u64 = 0;
    loop {
        let replaced = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if replaced != 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        let transient = matches!(
            error.raw_os_error(),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)
        );
        if !transient || attempt >= 50 {
            return Err(error);
        }
        attempt += 1;
        std::thread::sleep(std::time::Duration::from_millis(2 * attempt.min(10)));
    }
}

/// Read and decode a document, falling back to the default when it is missing
/// or unreadable.
///
/// A corrupt file is backed up rather than deleted — it may be the only copy
/// of something the user typed, and silently discarding it is worse than an
/// unexplained extra file.
pub fn read_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = std::fs::read_to_string(path) else {
        return T::default();
    };
    match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{} is not valid JSON ({e}); backing it up", path.display());
            let backup = path.with_extension(format!(
                "json.corrupt.{}.{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            if let Err(error) = std::fs::rename(path, &backup) {
                tracing::warn!(
                    "could not preserve corrupt file {} as {}: {error}",
                    path.display(),
                    backup.display()
                );
            }
            T::default()
        }
    }
}

/// Write a document atomically.
pub fn write<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // A fixed sibling temp name lets concurrent settings/workspace writes
    // steal one another's file. Give each writer its own name; the final
    // rename remains atomic, and a crash still leaves the previous document
    // untouched.
    let tmp = path.with_extension(format!(
        "json.tmp.{}.{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let write_result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = replace_file(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        // Persist the directory entry created by the atomic replacement.
        let directory = std::fs::File::open(parent)?;
        directory.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Doc {
        name: String,
        count: u32,
    }

    fn tmpdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hopperstore{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_reads_as_the_default() {
        let path = tmpdir().join("missing.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_or_default::<Doc>(&path), Doc::default());
    }

    #[test]
    fn a_document_round_trips() {
        let path = tmpdir().join("roundtrip.json");
        let doc = Doc {
            name: "shop".into(),
            count: 3,
        };
        write(&path, &doc).unwrap();
        assert_eq!(read_or_default::<Doc>(&path), doc);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_file_is_backed_up_rather_than_lost() {
        let path = tmpdir().join("corrupt.json");
        std::fs::write(&path, b"{not json").unwrap();

        assert_eq!(read_or_default::<Doc>(&path), Doc::default());
        let backups: Vec<_> = std::fs::read_dir(tmpdir())
            .unwrap()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                path.file_name()?
                    .to_str()?
                    .starts_with("corrupt.json.corrupt.")
                    .then_some(path)
            })
            .collect();
        assert_eq!(backups.len(), 1, "the unreadable document should be kept");
        assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), "{not json");
        let _ = std::fs::remove_file(&backups[0]);
    }

    #[test]
    fn repeated_corruption_keeps_each_recovery_copy() {
        let path = tmpdir().join("repeated-corrupt.json");
        std::fs::write(&path, b"{first").unwrap();
        assert_eq!(read_or_default::<Doc>(&path), Doc::default());
        std::fs::write(&path, b"{second").unwrap();
        assert_eq!(read_or_default::<Doc>(&path), Doc::default());

        let backups: Vec<_> = std::fs::read_dir(tmpdir())
            .unwrap()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                path.file_name()?
                    .to_str()?
                    .starts_with("repeated-corrupt.json.corrupt.")
                    .then_some(path)
            })
            .collect();
        assert_eq!(backups.len(), 2);
        for backup in backups {
            let _ = std::fs::remove_file(backup);
        }
    }

    #[test]
    fn writing_creates_missing_parent_directories() {
        let path = tmpdir().join("nested").join("deep").join("doc.json");
        let _ = std::fs::remove_dir_all(tmpdir().join("nested"));
        write(&path, &Doc::default()).unwrap();
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(tmpdir().join("nested"));
    }

    #[test]
    fn no_temporary_file_survives_a_successful_write() {
        let path = tmpdir().join("clean.json");
        write(&path, &Doc::default()).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn concurrent_writers_do_not_steal_each_others_temp_files() {
        let path = tmpdir().join("concurrent.json");
        let mut writers = Vec::new();
        for count in 0..8 {
            let path = path.clone();
            writers.push(std::thread::spawn(move || {
                write(
                    &path,
                    &Doc {
                        name: format!("writer-{count}"),
                        count,
                    },
                )
                .unwrap();
            }));
        }
        for writer in writers {
            writer.join().unwrap();
        }
        let result: Doc = read_or_default(&path);
        assert!(result.name.starts_with("writer-"));
        let _ = std::fs::remove_file(&path);
    }
}
