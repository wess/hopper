use engine::machines::media::Media;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
};

pub fn installer(url: String, data: &[u8]) -> Media {
  Media {
    language_code: "en-us".into(),
    architecture: "ARM64".into(),
    edition: "Professional".into(),
    size: data.len() as u64,
    sha1: sha1_smol::Sha1::from(data).digest().to_string(),
    file_path: url,
  }
}

pub async fn server(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}/installer", listener.local_addr().unwrap());
  let task = tokio::spawn(async move {
    let mut requests = Vec::new();
    for response in responses {
      let (mut socket, _) =
        tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
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

pub fn response(status: &str, headers: &str, data: &str) -> String {
  format!("HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n{data}")
}
