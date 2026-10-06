use anyhow::{bail, Context};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
pub async fn execute(socket: &Path, command: Value) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let stream = tokio::net::UnixStream::connect(socket)
            .await
            .context("The VM control socket is unavailable")?;
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        response(&mut reader, true).await?;
        writer
            .write_all(b"{\"execute\":\"qmp_capabilities\"}\r\n")
            .await?;
        response(&mut reader, false).await?;
        writer
            .write_all(format!("{command}\r\n").as_bytes())
            .await?;
        response(&mut reader, false).await?;
        Ok(())
    })
    .await
    .context("VM control timed out")?
}

#[cfg(not(unix))]
pub async fn execute(_: &Path, _: Value) -> anyhow::Result<()> {
    bail!("VM control requires a Unix host")
}

pub async fn input(socket: &Path, events: Vec<Value>) -> anyhow::Result<()> {
    execute(
        socket,
        json!({"execute":"input-send-event", "arguments":{"events":events}}),
    )
    .await
}

pub async fn screenshot(socket: &Path, path: &Path) -> anyhow::Result<()> {
    execute(
        socket,
        json!({"execute":"screendump", "arguments":{"filename":path,"format":"png"}}),
    )
    .await
}

async fn response(
    reader: &mut (impl AsyncBufReadExt + Unpin),
    greeting: bool,
) -> anyhow::Result<()> {
    loop {
        let mut line = Vec::new();
        let count = reader.take(65537).read_until(b'\n', &mut line).await?;
        if count == 0 {
            bail!("VM control connection closed");
        }
        if count > 65536 {
            bail!("VM control response was too large");
        }
        let value: Value = serde_json::from_slice(&line)?;
        if let Some(error) = value.get("error") {
            bail!("VM control failed: {error}");
        }
        if value.get(if greeting { "QMP" } else { "return" }).is_some() {
            return Ok(());
        }
    }
}

pub fn key_events(keys: &str) -> anyhow::Result<Vec<Value>> {
    let keys: Vec<_> = keys.split('+').collect();
    if keys.is_empty()
        || keys.len() > 8
        || keys.iter().any(|k| {
            k.is_empty()
                || k.len() > 32
                || !k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        })
    {
        bail!("Use QEMU key names separated by +, such as ctrl+alt+delete");
    }
    Ok(keys.iter().map(|key| (key,true)).chain(keys.iter().rev().map(|key| (key,false))).map(|(key,down)| json!({"type":"key","data":{"down":down,"key":{"type":"qcode","data":key}}})).collect())
}
