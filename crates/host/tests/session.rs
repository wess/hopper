//! Exercise switching at the socket boundary, including silent streams and
//! the gap between the create and start requests of one operation.

use docker::{Client, Endpoint};
use host::Host;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn request(stream: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
        let mut chunk = [0; 8192];
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() <= 64 * 1024);
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .next()
        .unwrap()
        .to_string()
}

fn endpoint(address: std::net::SocketAddr) -> Endpoint {
    Endpoint::Tcp {
        host: address.ip().to_string(),
        port: address.port(),
        tls: false,
    }
}

#[tokio::test]
async fn engine_switch_closes_a_silent_event_stream() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = Host::with_directory(
        Client::new(endpoint(listener.local_addr().unwrap())),
        tempfile::tempdir().unwrap().keep(),
    );
    let (ready_tx, ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert!(request(&mut socket).await.contains("/events"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await
            .unwrap();
        ready_tx.send(()).unwrap();
        let mut remainder = Vec::new();
        let closed =
            tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut remainder))
                .await
                .expect("switch left the old event socket alive");
        assert!(
            closed.is_ok()
                || matches!(
                    closed.unwrap_err().kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                )
        );
    });
    let event_host = Arc::clone(&host);
    let events = tokio::spawn(async move { event_host.stream_events(|_| true).await });
    ready.await.unwrap();
    host.set_endpoint(
        Endpoint::Unix {
            path: "/nonexistent-hopper-session.sock".into(),
        },
        "other",
        false,
    )
    .await;
    tokio::time::timeout(Duration::from_secs(2), events)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn switching_between_create_and_start_cannot_redirect_the_start() {
    let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = Host::with_directory(
        Client::new(endpoint(first.local_addr().unwrap())),
        tempfile::tempdir().unwrap().keep(),
    );
    let (created_tx, created) = tokio::sync::oneshot::channel();
    let (resume_tx, resume) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = first.accept().await.unwrap();
        assert!(request(&mut socket).await.contains("/containers/create"));
        created_tx.send(()).unwrap();
        resume.await.unwrap();
        let body = r#"{"Id":"created-on-first"}"#;
        socket
            .write_all(
                format!(
                    "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let (mut socket, _) = first.accept().await.unwrap();
        assert!(request(&mut socket)
            .await
            .contains("/containers/created-on-first/start"));
        socket
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let run_host = Arc::clone(&host);
    let run = tokio::spawn(async move {
        run_host
            .container_run(&model::RunInput {
                image: "fixture".into(),
                ..Default::default()
            })
            .await
    });
    created.await.unwrap();
    host.set_endpoint(endpoint(second.local_addr().unwrap()), "second", false)
        .await;
    resume_tx.send(()).unwrap();
    let id = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(id, "created-on-first");
    server.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), second.accept())
            .await
            .is_err()
    );
}
