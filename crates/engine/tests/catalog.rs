#![cfg(unix)]

use engine::machines::{media::catalogue, windows::catalog};
use reqwest::{Client, Url};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn entry() -> String {
  format!(
    "<File><LanguageCode>en-us</LanguageCode><Architecture>ARM64</Architecture>\
     <Edition>Professional</Edition><Size>4527171158</Size><Sha1>{}</Sha1>\
     <FilePath>http://dl.delivery.mp.microsoft.com/files/windows.esd</FilePath></File>",
    "a".repeat(40),
  )
}

fn xml(entries: &str) -> String {
  format!(
    "<MCT><Catalogs><Catalog><PublishedMedia><Files>{entries}</Files>\
     </PublishedMedia></Catalog></Catalogs></MCT>"
  )
}

#[test]
fn catalogue_rejects_ambiguous_selection_and_oversized_xml() {
  assert!(catalogue(&xml(&format!("{}{}", entry(), entry()))).is_err());
  assert!(catalogue(&" ".repeat(16 * 1024 * 1024 + 1)).is_err());
}

#[test]
fn catalogue_redirects_remain_on_official_https_origins() {
  for url in [
    catalog::ENDPOINT,
    "https://download.microsoft.com/windows.cab",
  ] {
    assert!(catalog::origin(&Url::parse(url).unwrap()));
  }
  for url in [
    "http://download.microsoft.com/windows.cab",
    "https://download.microsoft.com.evil.invalid/windows.cab",
    "https://example.com/windows.cab",
    "https://user@download.microsoft.com/windows.cab",
    "https://download.microsoft.com:8443/windows.cab",
  ] {
    assert!(!catalog::origin(&Url::parse(url).unwrap()));
  }
}

#[tokio::test]
async fn cab_decoding_bounds_helpers_and_preserves_source_identity() {
  let root = tempfile::tempdir().unwrap();
  let helper = root.path().join("archive");
  std::fs::write(&helper, "#!/bin/sh\ncat \"$0.xml\"\n").unwrap();
  std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
  std::fs::write(helper.with_extension("xml"), xml(&entry())).unwrap();
  let source = b"synthetic CAB bytes";
  let selection = catalog::decode(source, &helper).await.unwrap();
  assert_eq!(
    selection.media.file_path,
    "http://dl.delivery.mp.microsoft.com/files/windows.esd"
  );
  assert_eq!(
    selection.catalogue_sha256,
    format!("{:x}", Sha256::digest(source))
  );
  assert!(catalog::decode(&[], &helper).await.is_err());
  assert!(catalog::decode(&vec![0; 4 * 1024 * 1024 + 1], &helper)
    .await
    .is_err());
  std::fs::write(&helper, "#!/bin/sh\nhead -c 16777217 /dev/zero\n").unwrap();
  assert!(catalog::decode(source, &helper).await.is_err());
  std::fs::write(&helper, "#!/bin/sh\nexit 1\n").unwrap();
  assert!(catalog::decode(source, &helper).await.is_err());
}

async fn response(headers: &str, bytes: Vec<u8>) -> reqwest::Response {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let headers = headers.to_owned();
  tokio::spawn(async move {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = [0; 4096];
    if socket.read(&mut request).await.unwrap() == 0 {
      return;
    }
    if socket.write_all(headers.as_bytes()).await.is_ok() {
      let _ = socket.write_all(&bytes).await;
    }
  });
  Client::new()
    .get(format!("http://{address}"))
    .send()
    .await
    .unwrap()
}

#[tokio::test]
async fn catalogue_response_is_bounded_with_and_without_content_length() {
  let r = response(
    "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n",
    b"cab".to_vec(),
  )
  .await;
  assert_eq!(catalog::body(r).await.unwrap(), b"cab");
  let r = response("HTTP/1.1 200 OK\r\nContent-Length: 4194305\r\n\r\n", vec![]).await;
  assert!(catalog::body(r).await.is_err());
  let r = response(
    "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
    vec![0; 4194305],
  )
  .await;
  assert!(catalog::body(r).await.is_err());
  let r = response("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", vec![]).await;
  assert!(catalog::body(r).await.is_err());
  let r = response(
    "HTTP/1.1 500 Failed\r\nContent-Length: 3\r\n\r\n",
    b"bad".to_vec(),
  )
  .await;
  assert!(catalog::body(r).await.is_err());
}
