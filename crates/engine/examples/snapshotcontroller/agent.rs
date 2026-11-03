use anyhow::{ensure, Context};
use engine::machines::native::sessions::Restored;
use serde_json::{json, Value};
use std::{ffi::OsStr, path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub async fn restore(
  root: &Path,
  id: &str,
  snapshot: &str,
  executable: &OsStr,
) -> anyhow::Result<Restored> {
  let mut child = tokio::process::Command::new(executable)
    .arg("--stdio")
    .env("HOPPER_DIR", root)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()?;
  let mut input = child.stdin.take().context("Missing MCP diagnostic stdin")?;
  let output = child
    .stdout
    .take()
    .context("Missing MCP diagnostic stdout")?;
  let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
    "params":{"name":"vm.restore","arguments":{"id":id,"snapshot":snapshot}}});
  input.write_all(format!("{request}\n").as_bytes()).await?;
  let mut output = BufReader::new(output.take(1 << 20));
  let mut line = String::new();
  tokio::time::timeout(Duration::from_secs(915), output.read_line(&mut line)).await??;
  let reply: Value = serde_json::from_str(&line)?;
  ensure!(
    reply["id"] == 1 && reply.get("error").is_none() && reply["result"]["isError"] != true,
    "MCP restore failed: {reply}"
  );
  let receipt: Value = serde_json::from_str(
    reply["result"]["content"][0]["text"]
      .as_str()
      .context("Missing MCP restore receipt")?,
  )?;
  let value = |field: &str| -> anyhow::Result<String> {
    let value = receipt[field]
      .as_str()
      .context("Incomplete MCP restore receipt")?;
    model::native::machine_identity(value).context("Invalid MCP restore identity")?;
    Ok(value.into())
  };
  let restored = Restored {
    snapshot_id: value("snapshotId")?,
    rollback_id: value("rollbackId")?,
    generation_id: value("generationId")?,
  };
  ensure!(
    restored.snapshot_id == snapshot && restored.rollback_id != snapshot,
    "MCP restore lost checkpoint binding"
  );
  drop(input);
  ensure!(
    tokio::time::timeout(Duration::from_secs(5), child.wait())
      .await??
      .success(),
    "MCP diagnostic did not exit cleanly"
  );
  println!("Signed real-worker restore completed through MCP stdio, host facade and HPA1 with a fresh prior/generation receipt");
  Ok(restored)
}
