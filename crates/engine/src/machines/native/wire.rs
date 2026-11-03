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
        (Command::Start { .. }, Reply::Started { .. })
          | (Command::StartRestored { .. }, Reply::Restored { .. })
          | (
            Command::StartGeneration { .. },
            Reply::GenerationStarted { .. }
          )
          | (Command::RestoreResume { .. }, Reply::Running {})
          | (Command::RestorePause { .. }, Reply::Paused {})
          | (Command::Capture {}, Reply::Frame { .. })
          | (Command::Status {}, Reply::Status { .. })
          | (Command::Pause {}, Reply::Paused {})
          | (Command::Resume {}, Reply::Running {})
          | (
            Command::Input { .. } | Command::Release {} | Command::FinishDeployment {},
            Reply::Accepted {}
          )
          | (
            Command::SnapshotStart { .. } | Command::SnapshotCancel { .. },
            Reply::Accepted {}
          )
          | (Command::SnapshotPoll { .. }, Reply::Snapshot { .. })
          | (Command::Stop {}, Reply::Stopped { .. })
      ),
    "Unexpected native reply type"
  );
  if let (
    Command::SnapshotPoll { snapshot_id, .. },
    Reply::Snapshot {
      snapshot_id: returned,
      ..
    },
  ) = (command, &response.result)
  {
    ensure!(
      snapshot_id == returned && model::native::machine_identity(returned).is_some(),
      "Native snapshot reply identity differs"
    );
  }
  if let (
    Command::Start { boot },
    Reply::Started {
      machine_id,
      configuration,
    },
  ) = (command, &response.result)
  {
    ensure!(
      machine_id == &boot.machine_id && model::native::machine_identity(machine_id).is_some(),
      "Native worker started a different VM identity"
    );
    ensure!(
      *configuration != [0; 32],
      "Native worker omitted hardware configuration"
    );
  }
  if let (
    Command::StartRestored { boot, restore } | Command::StartGeneration { boot, restore },
    Reply::Restored {
      machine_id,
      configuration,
      restore: returned,
    }
    | Reply::GenerationStarted {
      machine_id,
      configuration,
      restore: returned,
    },
  ) = (command, &response.result)
  {
    ensure!(
      machine_id == &boot.machine_id
        && model::native::machine_identity(machine_id).is_some()
        && restore == returned
        && model::native::machine_identity(&returned.snapshot_id).is_some()
        && model::native::machine_identity(&returned.generation_id).is_some()
        && *configuration != [0; 32],
      "Restored worker identity/policy/configuration differs"
    );
  }
  Ok(())
}
