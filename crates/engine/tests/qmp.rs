#![cfg(unix)]
#[path = "../src/machines/qmp.rs"]
mod qmp;

use qmp::*;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn a_chord_releases_keys_in_reverse_order() {
    let events = key_events("ctrl+alt+delete").unwrap();
    assert_eq!(events.len(), 6);
    assert_eq!(events[0]["data"]["key"]["data"], "ctrl");
    assert_eq!(events[3]["data"]["key"]["data"], "delete");
    assert_eq!(events[5]["data"]["down"], false);
    assert!(key_events("ctrl++delete").is_err());
    assert!(key_events("../../other").is_err());
}

#[tokio::test]
async fn qmp_negotiation_skips_events_and_reports_input_errors() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("qmp.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        writer.write_all(b"{\"QMP\":{}}\r\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["execute"],
            "qmp_capabilities"
        );
        writer
            .write_all(b"{\"event\":\"RESUME\"}\r\n{\"return\":{}}\r\n")
            .await
            .unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["execute"],
            "input-send-event"
        );
        writer
            .write_all(b"{\"error\":{\"desc\":\"Unknown key\"}}\r\n")
            .await
            .unwrap();
    });
    let error = input(&socket, key_events("fakekey").unwrap())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Unknown key"));
    server.await.unwrap();
}
