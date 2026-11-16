//! Bounded build-context production on a blocking worker. Docker ignore rules
//! apply in file order with inherited per-pattern ancestor state.

use crate::{DockerError, Result};
use bytes::Bytes;
use model::BuildInput;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

mod ignore;
use self::ignore::load_rules;
pub use self::ignore::{is_excluded, load_dockerignore, Rules};

pub fn write_context(input: &BuildInput, writer: impl Write, cancelled: &AtomicBool) -> Result<()> {
    let context = PathBuf::from(&input.context_dir)
        .canonicalize()
        .map_err(|e| {
            DockerError::transport(format!(
                "Build context is not a directory or cannot be read: {e}"
            ))
        })?;
    if !context.is_dir() {
        return Err(DockerError::transport(format!(
            "{} is not a directory.",
            context.display()
        )));
    }
    let dockerfile = input.dockerfile_name();
    let dockerpath = Path::new(dockerfile);
    if dockerpath.is_absolute()
        || dockerpath
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(DockerError::decode(
            "The Dockerfile must be inside the selected build context.",
        ));
    }
    if !context.join(dockerfile).is_file() {
        return Err(DockerError::transport(format!(
            "No {dockerfile} in {}.",
            context.display()
        )));
    }
    let rules = Arc::new(load_rules(&context, dockerfile)?);
    let filter_rules = Arc::clone(&rules);
    let filter_context = context.clone();
    let filter_dockerfile = dockerfile.to_string();
    let walker = ::ignore::WalkBuilder::new(&context)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        .hidden(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            if !entry.file_type().is_some_and(|t| t.is_dir()) || filter_rules.negated {
                return true;
            }
            let Ok(relative) = entry.path().strip_prefix(&filter_context) else {
                return false;
            };
            if relative.as_os_str().is_empty()
                || Path::new(&filter_dockerfile).starts_with(relative)
            {
                return true;
            }
            !is_excluded(&filter_rules, relative, true, &filter_dockerfile)
        })
        .build();
    let mut archive = tar::Builder::new(writer);
    archive.follow_symlinks(false);
    for entry in walker {
        if cancelled.load(Ordering::Acquire) {
            return Err(DockerError::transport("Build context cancelled."));
        }
        let entry = entry.map_err(|e| {
            DockerError::transport(format!("Could not traverse build context: {e}"))
        })?;
        let relative = entry
            .path()
            .strip_prefix(&context)
            .map_err(|_| DockerError::decode("A context entry escaped its root."))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let directory = entry.file_type().is_some_and(|t| t.is_dir());
        if is_excluded(&rules, relative, directory, dockerfile) {
            continue;
        }
        archive
            .append_path_with_name(entry.path(), relative)
            .map_err(|e| {
                DockerError::transport(format!(
                    "Could not add {} to the build context: {e}",
                    relative.display()
                ))
            })?;
    }
    archive
        .finish()
        .map_err(|e| DockerError::transport(format!("Could not finish build context: {e}")))
}

/// A small compatibility helper for parser tests and small callers. Production
/// builds use stream_context and never assemble the archive in memory.
pub fn tar_context(input: &BuildInput) -> Result<Bytes> {
    let mut bytes = Vec::new();
    write_context(input, &mut bytes, &AtomicBool::new(false))?;
    Ok(bytes.into())
}

pub struct Cancel(pub Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

struct Writer {
    sender: tokio::sync::mpsc::Sender<std::io::Result<Bytes>>,
    cancel: Arc<AtomicBool>,
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for bytes in bytes.chunks(8192) {
            if self.cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Build cancelled.",
                ));
            }
            self.sender
                .blocking_send(Ok(Bytes::copy_from_slice(bytes)))
                .map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Build upload closed.")
                })?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct ContextStream {
    pub receiver: tokio::sync::mpsc::Receiver<std::io::Result<Bytes>>,
    pub worker: tokio::task::JoinHandle<Result<()>>,
    pub cancel: Cancel,
    pub finished: Arc<AtomicBool>,
}

pub fn stream_context(input: BuildInput) -> ContextStream {
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    let cancel = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let worker_finished = Arc::clone(&finished);
    let worker = tokio::task::spawn_blocking(move || {
        let writer = Writer {
            sender: sender.clone(),
            cancel: Arc::clone(&worker_cancel),
        };
        let result = write_context(&input, writer, &worker_cancel);
        worker_finished.store(true, Ordering::Release);
        if let Err(error) = &result {
            let _ = sender.blocking_send(Err(std::io::Error::other(error.message.clone())));
        }
        result
    });
    ContextStream {
        receiver,
        worker,
        cancel: Cancel(cancel),
        finished,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("Dockerfile"),
            "FROM scratch\nCOPY . /\n",
        )
        .unwrap();
        directory
    }

    #[test]
    fn negated_parent_rules_and_dockerfile_specific_rules_follow_docker_order() {
        let directory = fixture();
        std::fs::write(
            directory.path().join(".dockerignore"),
            "tree/file\n!tree\n/root.txt/\n",
        )
        .unwrap();
        let rules = load_dockerignore(directory.path()).unwrap();
        assert!(is_excluded(
            &rules,
            Path::new("tree/file"),
            false,
            "Dockerfile"
        ));
        assert!(is_excluded(
            &rules,
            Path::new("root.txt"),
            false,
            "Dockerfile"
        ));
        assert!(!is_excluded(
            &rules,
            Path::new("nested/root.txt"),
            false,
            "Dockerfile"
        ));
        std::fs::write(directory.path().join("Dockerfile.dockerignore"), "other\n").unwrap();
        let specific = load_dockerignore(directory.path()).unwrap();
        assert!(!is_excluded(
            &specific,
            Path::new("root.txt"),
            false,
            "Dockerfile"
        ));
        assert!(is_excluded(
            &specific,
            Path::new("other"),
            false,
            "Dockerfile"
        ));
    }

    #[test]
    fn malformed_ignore_files_fail_closed_and_unrelated_ignore_files_do_not_apply() {
        let directory = fixture();
        std::fs::write(directory.path().join(".dockerignore"), "[\n").unwrap();
        assert!(load_dockerignore(directory.path()).is_err());
        std::fs::write(directory.path().join(".dockerignore"), "").unwrap();
        std::fs::write(directory.path().join(".ignore"), "needed\n").unwrap();
        std::fs::write(directory.path().join("needed"), "included").unwrap();
        let input = BuildInput {
            context_dir: directory.path().to_string_lossy().into(),
            ..Default::default()
        };
        let bytes = tar_context(&input).unwrap();
        let mut archive = tar::Archive::new(std::io::Cursor::new(bytes));
        assert!(archive
            .entries()
            .unwrap()
            .any(|e| e.unwrap().path().unwrap() == Path::new("needed")));
    }

    #[tokio::test]
    async fn upload_backpressure_is_bounded_and_cancellation_stops_the_worker() {
        let directory = fixture();
        let file = std::fs::File::create(directory.path().join("large")).unwrap();
        file.set_len(32 * 1024 * 1024).unwrap();
        let ContextStream {
            mut receiver,
            worker,
            cancel,
            finished,
        } = stream_context(BuildInput {
            context_dir: directory.path().to_string_lossy().into(),
            ..Default::default()
        });
        let first = receiver.recv().await.unwrap().unwrap();
        assert!(first.len() <= 8192);
        assert_eq!(receiver.max_capacity(), 8);
        assert!(!finished.load(Ordering::Acquire));
        drop(cancel);
        drop(receiver);
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        assert!(finished.load(Ordering::Acquire));
    }
}
