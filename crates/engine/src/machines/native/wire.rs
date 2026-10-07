use anyhow::ensure;
use machine::ipc;
use model::native::{Command, Response, Result as Reply};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) struct Packet {
  pub response: Response,
  pub pixels: Vec<u8>,
}

pub(super) async fn read(reader: &mut (impl AsyncRead + Unpin)) -> anyhow::Result<Packet> {
  let mut prefix = [0; 8];
  reader.read_exact(&mut prefix).await?;
  let mut header = vec![0; ipc::header_length(&prefix)?];
  reader.read_exact(&mut header).await?;
  let response: Response = serde_json::from_slice(&header)?;
  let mut pixels = vec![0; ipc::payload_size(&response)?];
  reader.read_exact(&mut pixels).await?;
  Ok(Packet { response, pixels })
}

pub(super) async fn write(
  writer: &mut (impl AsyncWrite + Unpin),
  id: u64,
  command: &Command,
) -> anyhow::Result<()> {
  #[derive(serde::Serialize)]
  struct Outgoing<'a> {
    id: u64,
    command: &'a Command,
  }
  writer
    .write_all(&ipc::encode(&Outgoing { id, command }, &[])?)
    .await?;
  writer.flush().await?;
  Ok(())
}

pub(super) fn validate(id: u64, command: &Command, response: &Response) -> anyhow::Result<()> {
  ensure!(response.id == id, "Native reply does not match its request");
  ensure!(
    matches!(response.result, Reply::Rejected { .. })
      || matches!(
        (command, &response.result),
        (Command::Start { .. }, Reply::Started {})
          | (Command::Capture {}, Reply::Frame { .. })
          | (Command::Status {}, Reply::Status { .. })
          | (Command::Pause {}, Reply::Paused {})
          | (Command::Resume {}, Reply::Running {})
          | (
            Command::Input { .. } | Command::Release {},
            Reply::Accepted {}
          )
          | (Command::Stop {}, Reply::Stopped { .. })
      ),
    "Unexpected native reply type"
  );
  Ok(())
}
