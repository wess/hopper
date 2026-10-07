#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::linux::{download, preparation::Phase};
use sha2::{Digest, Sha256};
use std::{
  os::unix::fs::PermissionsExt,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
};

fn cache() -> tempfile::TempDir {
  let root = tempfile::tempdir().unwrap();
  std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
  root
}

fn digest(bytes: &[u8]) -> String {
  format!("{:x}", Sha256::digest(bytes))
}

fn write(path: &std::path::Path, bytes: &[u8]) {
  std::fs::write(path, bytes).unwrap();
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

async fn server(
  status: &str,
  headers: &str,
  bytes: &[u8],
  revoke: Option<Arc<AtomicBool>>,
) -> (String, tokio::task::JoinHandle<String>) {
  let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}/installer.iso", socket.local_addr().unwrap());
  let reply = format!("HTTP/1.1 {status}\r\n{headers}\r\nConnection: close\r\n\r\n");
  let bytes = bytes.to_vec();
  let task = tokio::spawn(async move {
    let (mut stream, _) = socket.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
      request.push(stream.read_u8().await.unwrap());
      assert!(request.len() < 4096);
    }
    if let Some(policy) = revoke {
      policy.store(false, Ordering::SeqCst);
    }
    stream.write_all(reply.as_bytes()).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    String::from_utf8(request).unwrap()
  });
  (url, task)
}

#[tokio::test]
async fn interrupted_media_resumes_and_verified_cache_needs_no_network() {
  let root = cache();
  let bytes = b"0123456789";
  let checksum = digest(bytes);
  let partial = root.path().join(format!("{checksum}.part"));
  let client = reqwest::Client::new();
  let (progress, status) = tokio::sync::watch::channel(Phase::Inspecting);
  let (url, task) = server("200 OK", "Content-Length: 10", &bytes[..4], None).await;
  assert!(download::tracked(
    &client,
    &url,
    10,
    &checksum,
    root.path(),
    Arc::new(|| Ok(())),
    &progress
  )
  .await
  .is_err());
  task.await.unwrap();
  assert_eq!(std::fs::read(&partial).unwrap(), &bytes[..4]);
  assert_eq!(
    *status.borrow(),
    Phase::Downloading {
      bytes: 4,
      total: 10
    }
  );
  let (url, task) = server(
    "206 Partial Content",
    "Content-Length: 6\r\nContent-Range: bytes 4-9/10",
    &bytes[4..],
    None,
  )
  .await;
  let complete = download::tracked(
    &client,
    &url,
    10,
    &checksum,
    root.path(),
    Arc::new(|| Ok(())),
    &progress,
  )
  .await
  .unwrap();
  assert!(task
    .await
    .unwrap()
    .to_lowercase()
    .contains("range: bytes=4-"));
  assert_eq!(std::fs::read(&complete).unwrap(), bytes);
  assert_eq!(
    *status.borrow(),
    Phase::Downloading {
      bytes: 10,
      total: 10
    }
  );
  assert!(!partial.exists());
  assert_eq!(
    std::fs::metadata(&complete).unwrap().permissions().mode() & 0o777,
    0o600
  );
  assert_eq!(
    download::tracked(
      &client,
      &url,
      10,
      &checksum,
      root.path(),
      Arc::new(|| Ok(())),
      &progress
    )
    .await
    .unwrap(),
    complete
  );
  assert_eq!(
    *status.borrow(),
    Phase::Verifying {
      bytes: 10,
      total: 10
    }
  );
}

#[tokio::test]
async fn ignored_range_restarts_and_bad_range_preserves_partial() {
  for valid in [false, true] {
    let root = cache();
    let bytes = b"0123456789";
    let checksum = digest(bytes);
    let partial = root.path().join(format!("{checksum}.part"));
    write(&partial, &bytes[..4]);
    let (url, task) = if valid {
      server("200 OK", "Content-Length: 10", bytes, None).await
    } else {
      server(
        "206 Partial Content",
        "Content-Length: 6\r\nContent-Range: bytes 5-10/11",
        &bytes[4..],
        None,
      )
      .await
    };
    let result = download::fetch(
      &reqwest::Client::new(),
      &url,
      10,
      &checksum,
      root.path(),
      Arc::new(|| Ok(())),
    )
    .await;
    task.await.unwrap();
    if valid {
      assert_eq!(std::fs::read(result.unwrap()).unwrap(), bytes);
    } else {
      assert!(result.is_err());
      assert_eq!(std::fs::read(partial).unwrap(), &bytes[..4]);
    }
  }
}

#[tokio::test]
async fn checksum_failure_does_not_publish_and_retry_can_restart() {
  let root = cache();
  let checksum = digest(b"correct");
  let (url, task) = server("200 OK", "Content-Length: 7", b"corrupt", None).await;
  assert!(download::fetch(
    &reqwest::Client::new(),
    &url,
    7,
    &checksum,
    root.path(),
    Arc::new(|| Ok(()))
  )
  .await
  .is_err());
  task.await.unwrap();
  assert!(!root.path().join(format!("{checksum}.iso")).exists());
  assert_eq!(
    std::fs::metadata(root.path().join(format!("{checksum}.part")))
      .unwrap()
      .len(),
    0
  );
}

#[tokio::test]
async fn revocation_before_body_preserves_partial_and_prevents_publication() {
  let root = cache();
  let bytes = b"0123456789";
  let checksum = digest(bytes);
  let partial = root.path().join(format!("{checksum}.part"));
  write(&partial, &bytes[..4]);
  let policy = Arc::new(AtomicBool::new(true));
  let (url, task) = server(
    "206 Partial Content",
    "Content-Length: 6\r\nContent-Range: bytes 4-9/10",
    &bytes[4..],
    Some(policy.clone()),
  )
  .await;
  let check = Arc::new(move || {
    anyhow::ensure!(policy.load(Ordering::SeqCst), "revoked");
    Ok(())
  });
  assert!(download::fetch(
    &reqwest::Client::new(),
    &url,
    10,
    &checksum,
    root.path(),
    check
  )
  .await
  .is_err());
  task.await.unwrap();
  assert_eq!(std::fs::read(partial).unwrap(), &bytes[..4]);
  assert!(!root.path().join(format!("{checksum}.iso")).exists());
}

#[tokio::test]
async fn corrupt_or_symlink_cache_is_preserved_and_rejected() {
  let root = cache();
  let checksum = digest(b"correct");
  let complete = root.path().join(format!("{checksum}.iso"));
  write(&complete, b"corrupt");
  assert!(download::fetch(
    &reqwest::Client::new(),
    "http://127.0.0.1:1/installer.iso",
    7,
    &checksum,
    root.path(),
    Arc::new(|| Ok(()))
  )
  .await
  .is_err());
  assert_eq!(std::fs::read(&complete).unwrap(), b"corrupt");
  let preserved = root.path().join("preserved");
  std::fs::rename(&complete, &preserved).unwrap();
  std::os::unix::fs::symlink(&preserved, &complete).unwrap();
  assert!(download::fetch(
    &reqwest::Client::new(),
    "http://127.0.0.1:1/installer.iso",
    7,
    &checksum,
    root.path(),
    Arc::new(|| Ok(()))
  )
  .await
  .is_err());
  assert_eq!(std::fs::read(preserved).unwrap(), b"corrupt");
  assert!(std::fs::symlink_metadata(complete)
    .unwrap()
    .file_type()
    .is_symlink());
}
