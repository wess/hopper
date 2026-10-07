#[path = "support/download.rs"]
mod wire;
use engine::machines::media::download;
use reqwest::Client;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
};
use wire::{installer, response, server};

#[tokio::test]
async fn interrupted_transfer_resumes_real_http_bytes_and_publishes_only_verified_media() {
  let data = b"0123456789";
  let (url, task) = server(vec![
    response("200 OK", "Content-Length: 10\r\n", "0123"),
    response(
      "206 Partial Content",
      "Content-Length: 6\r\nContent-Range: bytes 4-9/10\r\n",
      "456789",
    ),
  ])
  .await;
  let dir = tempfile::tempdir().unwrap();
  let media = installer(url, data);
  let progress = dir.path().join("progress");
  let client = Client::new();
  assert!(download::fetch(&client, &media, dir.path(), &progress)
    .await
    .is_err());
  assert!(!dir.path().join(format!("{}.esd", media.sha1)).exists());
  assert_eq!(
    std::fs::read(dir.path().join(format!("{}.esd.part", media.sha1))).unwrap(),
    b"0123"
  );
  let path = download::fetch(&client, &media, dir.path(), &progress)
    .await
    .unwrap();
  assert_eq!(std::fs::read(path).unwrap(), data);
  assert!(!dir.path().join(format!("{}.esd.part", media.sha1)).exists());
  let requests = task.await.unwrap();
  assert!(!requests[0].contains("range:"));
  assert!(requests[1].contains("range: bytes=4-"));
  assert!(requests[1].contains("accept-encoding: identity"));
}

#[tokio::test]
async fn server_ignoring_range_replaces_the_prefix_instead_of_appending() {
  let (url, task) = server(vec![response(
    "200 OK",
    "Content-Length: 10\r\n",
    "0123456789",
  )])
  .await;
  let dir = tempfile::tempdir().unwrap();
  let media = installer(url, b"0123456789");
  std::fs::write(
    dir.path().join(format!("{}.esd.part", media.sha1)),
    b"wrong",
  )
  .unwrap();
  let path = download::fetch(
    &Client::new(),
    &media,
    dir.path(),
    &dir.path().join("progress"),
  )
  .await
  .unwrap();
  assert_eq!(std::fs::read(path).unwrap(), b"0123456789");
  assert!(task.await.unwrap()[0].contains("range: bytes=5-"));
}

#[tokio::test]
async fn invalid_resume_headers_preserve_the_prefix_and_never_publish() {
  for headers in [
    "Content-Length: 6\r\nContent-Range: bytes 3-8/10\r\n",
    "Content-Length: 6\r\nContent-Range: bytes 4-9/11\r\n",
    "Content-Length: 5\r\nContent-Range: bytes 4-9/10\r\n",
    "Content-Length: 6\r\n",
    "Content-Length: 6\r\nContent-Range: bytes 4-9/10\r\nContent-Encoding: gzip\r\n",
  ] {
    let (url, task) = server(vec![response("206 Partial Content", headers, "456789")]).await;
    let dir = tempfile::tempdir().unwrap();
    let media = installer(url, b"0123456789");
    let partial = dir.path().join(format!("{}.esd.part", media.sha1));
    std::fs::write(&partial, b"0123").unwrap();
    assert!(download::fetch(
      &Client::new(),
      &media,
      dir.path(),
      &dir.path().join("progress")
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(&partial).unwrap(), b"0123");
    assert!(!dir.path().join(format!("{}.esd", media.sha1)).exists());
    task.await.unwrap();
  }
}

#[tokio::test]
async fn checksum_failure_discards_bad_prefix_and_next_attempt_starts_fresh() {
  let (url, task) = server(vec![
    response("200 OK", "Content-Length: 10\r\n", "bad3456789"),
    response("200 OK", "Content-Length: 10\r\n", "0123456789"),
  ])
  .await;
  let dir = tempfile::tempdir().unwrap();
  let media = installer(url, b"0123456789");
  let client = Client::new();
  let progress = dir.path().join("progress");
  assert!(download::fetch(&client, &media, dir.path(), &progress)
    .await
    .is_err());
  assert_eq!(
    std::fs::metadata(dir.path().join(format!("{}.esd.part", media.sha1)))
      .unwrap()
      .len(),
    0
  );
  let path = download::fetch(&client, &media, dir.path(), &progress)
    .await
    .unwrap();
  assert_eq!(std::fs::read(path).unwrap(), b"0123456789");
  assert!(task
    .await
    .unwrap()
    .iter()
    .all(|request| !request.contains("range:")));
}

#[tokio::test]
async fn verified_cache_and_complete_partial_need_no_network() {
  for suffix in ["esd", "esd.part"] {
    let dir = tempfile::tempdir().unwrap();
    let media = installer("http://127.0.0.1:0/unreachable".into(), b"0123456789");
    std::fs::write(
      dir.path().join(format!("{}.{suffix}", media.sha1)),
      b"0123456789",
    )
    .unwrap();
    let path = download::fetch(
      &Client::new(),
      &media,
      dir.path(),
      &dir.path().join("progress"),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"0123456789");
  }
}

#[tokio::test]
async fn corrupt_cache_is_replaced_only_after_a_verified_download() {
  let (url, task) = server(vec![response(
    "200 OK",
    "Content-Length: 10\r\n",
    "0123456789",
  )])
  .await;
  let dir = tempfile::tempdir().unwrap();
  let media = installer(url, b"0123456789");
  let path = dir.path().join(format!("{}.esd", media.sha1));
  std::fs::write(&path, b"bad3456789").unwrap();
  assert_eq!(
    download::fetch(
      &Client::new(),
      &media,
      dir.path(),
      &dir.path().join("progress")
    )
    .await
    .unwrap(),
    path
  );
  assert_eq!(std::fs::read(path).unwrap(), b"0123456789");
  task.await.unwrap();
}

#[tokio::test]
async fn concurrent_download_cannot_modify_a_locked_partial() {
  use fs2::FileExt;
  let dir = tempfile::tempdir().unwrap();
  let media = installer("http://127.0.0.1:0/unreachable".into(), b"0123456789");
  let path = dir.path().join(format!("{}.esd.part", media.sha1));
  std::fs::write(&path, b"0123").unwrap();
  let file = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(&path)
    .unwrap();
  file.lock_exclusive().unwrap();
  assert!(download::fetch(
    &Client::new(),
    &media,
    dir.path(),
    &dir.path().join("progress")
  )
  .await
  .unwrap_err()
  .to_string()
  .contains("Another VM"));
  assert_eq!(std::fs::read(path).unwrap(), b"0123");
}

#[tokio::test]
async fn cancelled_transfer_retains_bytes_for_the_next_request() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}/installer", listener.local_addr().unwrap());
  let server = tokio::spawn(async move {
    let (mut first, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
      request.push(first.read_u8().await.unwrap());
    }
    first
      .write_all(response("200 OK", "Content-Length: 10\r\n", "0123").as_bytes())
      .await
      .unwrap();
    assert_eq!(first.read(&mut [0; 1]).await.unwrap(), 0);
    let (mut second, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
      request.push(second.read_u8().await.unwrap());
    }
    assert!(String::from_utf8(request)
      .unwrap()
      .contains("range: bytes=4-"));
    second
      .write_all(
        response(
          "206 Partial Content",
          "Content-Length: 6\r\nContent-Range: bytes 4-9/10\r\n",
          "456789",
        )
        .as_bytes(),
      )
      .await
      .unwrap();
  });
  let dir = tempfile::tempdir().unwrap();
  let media = installer(url.clone(), b"0123456789");
  let root = dir.path().to_owned();
  let first = tokio::spawn(async move {
    download::fetch(&Client::new(), &media, &root, &root.join("progress")).await
  });
  let media = installer(url, b"0123456789");
  let partial = dir.path().join(format!("{}.esd.part", media.sha1));
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0) != 4 {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
  first.abort();
  assert!(first.await.unwrap_err().is_cancelled());
  let path = download::fetch(
    &Client::new(),
    &media,
    dir.path(),
    &dir.path().join("progress"),
  )
  .await
  .unwrap();
  assert_eq!(std::fs::read(path).unwrap(), b"0123456789");
  tokio::time::timeout(std::time::Duration::from_secs(5), server)
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn oversized_body_without_a_length_never_exceeds_catalogue_bounds() {
  let (url, task) = server(vec![response("200 OK", "", "0123456789extra")]).await;
  let dir = tempfile::tempdir().unwrap();
  let media = installer(url, b"0123456789");
  assert!(download::fetch(
    &Client::new(),
    &media,
    dir.path(),
    &dir.path().join("progress")
  )
  .await
  .is_err());
  assert!(!dir.path().join(format!("{}.esd", media.sha1)).exists());
  assert!(
    std::fs::metadata(dir.path().join(format!("{}.esd.part", media.sha1)))
      .unwrap()
      .len()
      <= media.size
  );
  task.await.unwrap();
}
