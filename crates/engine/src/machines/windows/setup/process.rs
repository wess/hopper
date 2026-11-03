use anyhow::{ensure, Context};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) async fn run(
  program: &Path,
  args: &[String],
  input: Option<&[u8]>,
  output: Option<&Path>,
  limit: usize,
) -> anyhow::Result<Vec<u8>> {
  let mut file = if let Some(path) = output {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    Some(tokio::fs::File::from_std(options.open(path)?))
  } else {
    None
  };
  let mut child = tokio::process::Command::new(program)
    .args(args)
    .env("LC_ALL", "C")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .context("Launch Windows media helper")?;
  let mut stdin = child.stdin.take().context("Missing media helper input")?;
  let mut stdout = child.stdout.take().context("Missing media helper output")?;
  let mut stderr = child
    .stderr
    .take()
    .context("Missing media helper diagnostics")?;
  let result = tokio::time::timeout(Duration::from_secs(30 * 60), async {
    let writer = async {
      if let Some(bytes) = input {
        stdin.write_all(bytes).await?;
      }
      drop(stdin);
      Ok::<_, anyhow::Error>(())
    };
    let reader = async {
      let mut bytes = Vec::new();
      let mut total = 0usize;
      let mut buffer = vec![0; 64 * 1024];
      loop {
        let read = stdout.read(&mut buffer).await?;
        if read == 0 {
          break;
        }
        total = total
          .checked_add(read)
          .context("Media output size overflow")?;
        ensure!(total <= limit, "Media helper output exceeds its bound");
        if let Some(file) = &mut file {
          if let Some(path) = output {
            ensure!(
              fs2::available_space(path)? > 256 * 1024 * 1024 + read as u64,
              "Setup preparation needs more free disk space"
            );
          }
          file.write_all(&buffer[..read]).await?;
        } else {
          bytes.extend_from_slice(&buffer[..read]);
        }
      }
      if let Some(file) = &mut file {
        file.sync_all().await?;
      }
      Ok::<_, anyhow::Error>(bytes)
    };
    let diagnostics = async {
      let mut tail = Vec::with_capacity(4096);
      let mut truncated = false;
      let mut buffer = [0; 8192];
      loop {
        let read = stderr.read(&mut buffer).await?;
        if read == 0 {
          break;
        }
        truncated |= tail.len() + read > 4096;
        if read >= 4096 {
          tail.clear();
          tail.extend_from_slice(&buffer[read - 4096..read]);
        } else {
          let discard = (tail.len() + read).saturating_sub(4096);
          tail.drain(..discard);
          tail.extend_from_slice(&buffer[..read]);
        }
      }
      Ok::<_, anyhow::Error>((tail, truncated))
    };
    let (_, bytes, (diagnostics, truncated)) = tokio::try_join!(writer, reader, diagnostics)?;
    let status = child.wait().await?;
    if !status.success() {
      let text: String = String::from_utf8_lossy(&diagnostics)
        .chars()
        .filter(|value| !value.is_control() || matches!(value, '\n' | '\t'))
        .collect();
      anyhow::bail!(
        "Windows media helper failed ({status}): {}{}",
        if truncated {
          "[earlier diagnostics truncated] "
        } else {
          ""
        },
        text.trim()
      );
    }
    Ok::<_, anyhow::Error>(bytes)
  })
  .await
  .context("Windows media helper timed out")
  .and_then(|result| result);
  if result.is_err() {
    let _ = child.kill().await;
    let _ = child.wait().await;
  }
  result
}
