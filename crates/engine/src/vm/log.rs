//! Keep VM diagnostics bounded even when a child never stops writing.

use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const MAX_BYTES: u64 = 8 * 1024 * 1024;

pub struct Log {
    path: PathBuf,
    file: tokio::fs::File,
    bytes: u64,
}

async fn open(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path).await
}

impl Log {
    pub async fn open(path: PathBuf) -> std::io::Result<Self> {
        let file = open(&path).await?;
        let bytes = file.metadata().await?.len();
        Ok(Self { path, file, bytes })
    }

    pub async fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        if self.bytes.saturating_add(data.len() as u64) > MAX_BYTES {
            self.file.flush().await?;
            let previous = self.path.with_extension("log.1");
            match tokio::fs::remove_file(&previous).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            tokio::fs::rename(&self.path, previous).await?;
            self.file = open(&self.path).await?;
            self.bytes = 0;
        }
        // Pumps write 8 KiB chunks; retain a bounded tail for other callers.
        let data = &data[data.len().saturating_sub(MAX_BYTES as usize)..];
        self.file.write_all(data).await?;
        self.bytes += data.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn continuous_output_rotates_to_two_bounded_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("engine.log");
        let mut log = Log::open(path.clone()).await.unwrap();
        let chunk = vec![b'x'; 8192];
        for _ in 0..2200 {
            log.write(&chunk).await.unwrap();
        }
        assert!(tokio::fs::metadata(path).await.unwrap().len() <= MAX_BYTES);
        assert!(
            tokio::fs::metadata(directory.path().join("engine.log.1"))
                .await
                .unwrap()
                .len()
                <= MAX_BYTES
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}
