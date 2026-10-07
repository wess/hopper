#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::vz::mac::{
  download::{self, Source},
  Phase,
};
use machine::vz::queue::Check;
use std::{
  os::unix::fs::PermissionsExt,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::Duration,
};
fn response(status: &str, headers: &str, data: &str) -> String {
  format!("HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n{data}")
}

async fn server(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
  use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
  };
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}/restore", listener.local_addr().unwrap());
  let task = tokio::spawn(async move {
    let mut requests = Vec::new();
    for response in responses {
      let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
      let mut request = Vec::new();
      while !request.ends_with(b"\r\n\r\n") {
        assert!(request.len() < 8192);
        request.push(socket.read_u8().await.unwrap());
      }
      requests.push(String::from_utf8(request).unwrap());
      socket.write_all(response.as_bytes()).await.unwrap();
      socket.shutdown().await.unwrap();
    }
    requests
  });
  (url, task)
}

fn source(url: String) -> Source {
  Source {
    url,
    size: 10,
    etag: "\"fixture\"".into(),
  }
}
fn client() -> reqwest::Client {
  reqwest::Client::builder()
    .redirect(reqwest::redirect::Policy::none())
    .build()
    .unwrap()
}
fn check() -> Check {
  Arc::new(|| Ok(()))
}
fn cache() -> tempfile::TempDir {
  let root = tempfile::tempdir().unwrap();
  std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
  root
}
fn entry(root: &std::path::Path) -> std::path::PathBuf {
  std::fs::read_dir(root)
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path()
}

#[tokio::test]
async fn interrupted_download_resumes_with_frozen_etag_and_rechecks_cached_bytes() {
  let root = cache();
  let (url, task) = server(vec![
    response(
      "200 OK",
      "Content-Length: 10\r\nETag: \"fixture\"\r\n",
      "0123",
    ),
    response(
      "206 Partial Content",
      "Content-Length: 6\r\nContent-Range: bytes 4-9/10\r\nETag: \"fixture\"\r\n",
      "456789",
    ),
  ])
  .await;
  let source = source(url);
  let (progress, status) = tokio::sync::watch::channel(Phase::Inspecting);
  assert!(
    download::fetch(&client(), &source, root.path(), check(), &progress)
      .await
      .is_err()
  );
  let partial = entry(root.path()).join("restore.part");
  assert_eq!(std::fs::read(&partial).unwrap(), b"0123");
  let path = download::fetch(&client(), &source, root.path(), check(), &progress)
    .await
    .unwrap();
  assert_eq!(std::fs::read(&path).unwrap(), b"0123456789");
  let requests = task.await.unwrap();
  assert!(requests[1].to_ascii_lowercase().contains("range: bytes=4-"));
  assert!(requests[1]
    .to_ascii_lowercase()
    .contains("if-match: \"fixture\""));
  assert!(requests[1]
    .to_ascii_lowercase()
    .contains("if-range: \"fixture\""));
  assert!(!partial.exists());
  assert_eq!(
    download::fetch(&client(), &source, root.path(), check(), &progress)
      .await
      .unwrap(),
    path
  );
  assert_eq!(
    *status.borrow(),
    Phase::Verifying {
      bytes: 10,
      total: 10
    }
  );
  std::fs::write(&path, b"tampered!!").unwrap();
  assert!(
    download::fetch(&client(), &source, root.path(), check(), &progress)
      .await
      .unwrap_err()
      .to_string()
      .contains("verification")
  );
  assert_eq!(std::fs::read(path).unwrap(), b"tampered!!");
}

#[tokio::test]
async fn changed_etag_and_inexact_ranges_preserve_partial_media() {
  for headers in [
    "ETag: \"changed\"\r\nContent-Range: bytes 4-9/10\r\n",
    "ETag: \"fixture\"\r\nContent-Range: bytes 3-8/10\r\n",
  ] {
    let root = cache();
    let (url, task) = server(vec![
      response(
        "200 OK",
        "Content-Length: 10\r\nETag: \"fixture\"\r\n",
        "0123",
      ),
      response(
        "206 Partial Content",
        &format!("Content-Length: 6\r\n{headers}"),
        "456789",
      ),
    ])
    .await;
    let source = source(url);
    let (progress, _) = tokio::sync::watch::channel(Phase::Inspecting);
    assert!(
      download::fetch(&client(), &source, root.path(), check(), &progress)
        .await
        .is_err()
    );
    assert!(
      download::fetch(&client(), &source, root.path(), check(), &progress)
        .await
        .is_err()
    );
    task.await.unwrap();
    assert_eq!(
      std::fs::read(entry(root.path()).join("restore.part")).unwrap(),
      b"0123"
    );
    assert!(!entry(root.path()).join("restore.ipsw").exists());
  }
}

#[tokio::test]
async fn revoked_download_does_not_wait_for_stalled_http_response() {
  use tokio::{io::AsyncReadExt, net::TcpListener};
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let source = source(format!("http://{}/restore", listener.local_addr().unwrap()));
  let allowed = Arc::new(AtomicBool::new(true));
  let policy = allowed.clone();
  let check: Check = Arc::new(move || {
    anyhow::ensure!(policy.load(Ordering::Acquire), "original access revoked");
    Ok(())
  });
  let revoke = allowed.clone();
  let task = tokio::spawn(async move {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut byte = [0];
    socket.read_exact(&mut byte).await.unwrap();
    revoke.store(false, Ordering::Release);
    tokio::time::sleep(Duration::from_secs(2)).await;
  });
  let root = cache();
  let (progress, _) = tokio::sync::watch::channel(Phase::Inspecting);
  let error = tokio::time::timeout(
    Duration::from_secs(1),
    download::fetch(&client(), &source, root.path(), check, &progress),
  )
  .await
  .unwrap()
  .unwrap_err();
  assert!(error.to_string().contains("revoked"));
  assert!(!entry(root.path()).join("restore.ipsw").exists());
  task.abort();
}

#[tokio::test]
async fn symlinked_partial_cannot_read_or_change_its_target() {
  let root = cache();
  let (url, task) = server(vec![response(
    "200 OK",
    "Content-Length: 10\r\nETag: \"fixture\"\r\n",
    "0123",
  )])
  .await;
  let source = source(url);
  let (progress, _) = tokio::sync::watch::channel(Phase::Inspecting);
  assert!(
    download::fetch(&client(), &source, root.path(), check(), &progress)
      .await
      .is_err()
  );
  task.await.unwrap();
  let partial = entry(root.path()).join("restore.part");
  std::fs::remove_file(&partial).unwrap();
  let owned = tempfile::NamedTempFile::new().unwrap();
  std::fs::write(owned.path(), b"preserved").unwrap();
  std::os::unix::fs::symlink(owned.path(), partial).unwrap();
  assert!(
    download::fetch(&client(), &source, root.path(), check(), &progress)
      .await
      .is_err()
  );
  assert_eq!(std::fs::read(owned.path()).unwrap(), b"preserved");
}

#[test]
fn official_origins_reject_credentials_ports_fragments_and_lookalikes() {
  download::official("https://updates.cdn-apple.com/restore.ipsw").unwrap();
  for url in [
    "http://updates.cdn-apple.com/restore",
    "https://updates.cdn-apple.com.evil.test/restore",
    "https://user@updates.cdn-apple.com/restore",
    "https://updates.cdn-apple.com:8443/restore",
    "https://updates.cdn-apple.com/restore#fragment",
  ] {
    assert!(download::official(url).is_err(), "{url}");
  }
}

#[test]
fn local_restore_metadata_must_match_discovery_except_for_its_file_url() {
  use engine::machines::vz::mac::acquire;
  let image = machine::vz::restore::Image {
    url: "https://updates.cdn-apple.com/restore.ipsw".into(),
    build: "fixture".into(),
    version: [26, 0, 0],
    hardware: vec![1],
    minimum_cpus: 2,
    minimum_memory: 4 << 30,
  };
  let mut local = image.clone();
  local.url = "file:///owned/restore.ipsw".into();
  acquire::matches(&image, &local).unwrap();
  local.hardware = vec![2];
  assert!(acquire::matches(&image, &local).is_err());
  local = image.clone();
  local.build = "different".into();
  assert!(acquire::matches(&image, &local).is_err());
}
