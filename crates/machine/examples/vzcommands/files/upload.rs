use anyhow::ensure;
use machine::vz::queue::{Check, Client};
use model::guest::{
  files::{Read, Receipt, Transfer, Upload, Uploaded, CHUNK, CREATE_LIMIT},
  wire::{Action, Outcome},
  Command, Phase, Role,
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

async fn request(
  client: &Client,
  id: &str,
  check: &Check,
  action: Action,
) -> anyhow::Result<Transfer> {
  match client
    .guest_request(id, 1, 0, action, (check.clone(), check.clone()))
    .await?
  {
    Outcome::Transfer { transfer } => Ok(transfer),
    Outcome::Error { message } => anyhow::bail!("{message}"),
    Outcome::Job { .. } => anyhow::bail!("Unexpected job response to upload request"),
  }
}

pub(super) async fn check(client: &Client, id: &str, check: &Check) -> anyhow::Result<()> {
  let bytes = (0..3 * CREATE_LIMIT + 17)
    .map(|index| index as u8)
    .collect::<Vec<_>>();
  for (role, uid, name) in [
    (Role::Normal, 1001, "normal"),
    (Role::Administrator, 0, "admin"),
  ] {
    let path = format!("/home/hopper/{name}.upload");
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let file = Upload {
      path: path.clone(),
      size: bytes.len() as u64,
      sha256: sha256.clone(),
      timeout_seconds: 60,
      role,
    };
    let mut transfer = request(client, id, check, Action::BeginUpload { file }).await?;
    let deadline = Instant::now() + Duration::from_secs(70);
    while transfer.offset < transfer.size {
      ensure!(
        Instant::now() < deadline && !transfer.job.finished,
        "Upload stalled or failed"
      );
      let offset = transfer.offset as usize;
      let end = bytes.len().min(offset + CREATE_LIMIT);
      let before = transfer.offset;
      transfer = request(
        client,
        id,
        check,
        Action::UploadChunk {
          job: transfer.job.id.clone(),
          offset: before,
          bytes: bytes[offset..end].to_vec(),
        },
      )
      .await?;
      ensure!(
        transfer.offset >= before && transfer.offset <= end as u64,
        "Invalid accepted upload offset"
      );
      if transfer.offset == before {
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    }
    let status = request(
      client,
      id,
      check,
      Action::UploadStatus {
        job: transfer.job.id.clone(),
      },
    )
    .await?;
    ensure!(
      status.offset == transfer.size && !status.committed,
      "Upload committed before explicit request"
    );
    while !transfer.committed {
      ensure!(Instant::now() < deadline, "Upload commit stalled");
      transfer = request(
        client,
        id,
        check,
        Action::CommitUpload {
          job: transfer.job.id.clone(),
        },
      )
      .await?;
      if !transfer.committed {
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    }
    let job = super::finish(client, id, check, transfer.job).await?;
    ensure!(
      job.exit_code == Some(0)
        && job
          .identity
          .as_ref()
          .is_some_and(|identity| identity.uid == uid),
      "Upload identity or completion mismatch"
    );
    let uploaded: Uploaded = serde_json::from_slice(&job.stdout)?;
    ensure!(
      uploaded.sha256 == sha256 && uploaded.file.length == bytes.len(),
      "Upload receipt mismatch"
    );
    let mut offset = 0;
    while offset < bytes.len() {
      let read = Read {
        path: path.clone(),
        offset: offset as u64,
        length: CHUNK,
        stamp: Some(uploaded.file.stamp),
        role,
      };
      let job = super::run(
        client,
        id,
        check,
        Action::Start {
          command: read.command().unwrap(),
        },
      )
      .await?;
      let end = bytes.len().min(offset + CHUNK);
      ensure!(
        job.exit_code == Some(0) && job.stdout == bytes[offset..end],
        "Uploaded binary bytes did not round-trip"
      );
      let receipt: Receipt = serde_json::from_slice(&job.stderr)?;
      ensure!(
        receipt.stamp == uploaded.file.stamp && receipt.eof == (end == bytes.len()),
        "Upload read stamp or EOF mismatch"
      );
      offset = end;
    }
  }
  failure_checks(client, id, check).await
}

async fn failure_checks(client: &Client, id: &str, check: &Check) -> anyhow::Result<()> {
  for cancelled in [true, false] {
    let path = format!("/home/hopper/rejected-{cancelled}.upload");
    let file = Upload {
      path: path.clone(),
      size: 3,
      sha256: if cancelled {
        format!("{:x}", Sha256::digest(b"abc"))
      } else {
        "00".repeat(32)
      },
      timeout_seconds: 30,
      role: Role::Normal,
    };
    let transfer = request(client, id, check, Action::BeginUpload { file }).await?;
    let mut transfer = request(
      client,
      id,
      check,
      Action::UploadChunk {
        job: transfer.job.id,
        offset: 0,
        bytes: b"abc".to_vec(),
      },
    )
    .await?;
    ensure!(
      transfer.offset == 3 && !transfer.committed,
      "Small upload was not fully queued"
    );
    if cancelled {
      let job = super::run(
        client,
        id,
        check,
        Action::Cancel {
          job: transfer.job.id.clone(),
        },
      )
      .await?;
      ensure!(
        job.phase == Phase::Cancelled,
        "Uncommitted upload cancellation did not finish"
      );
      let status = request(
        client,
        id,
        check,
        Action::UploadStatus {
          job: transfer.job.id.clone(),
        },
      )
      .await?;
      ensure!(
        status.job.finished && !status.committed,
        "Cancelled upload acquired commit state"
      );
      let result = client
        .guest_request(
          id,
          1,
          0,
          Action::CommitUpload {
            job: transfer.job.id,
          },
          (check.clone(), check.clone()),
        )
        .await?;
      ensure!(
        matches!(result, Outcome::Error { .. }),
        "Cancelled upload accepted commit"
      );
    } else {
      let deadline = Instant::now() + Duration::from_secs(35);
      while !transfer.committed {
        ensure!(Instant::now() < deadline, "Corrupt upload commit stalled");
        transfer = request(
          client,
          id,
          check,
          Action::CommitUpload {
            job: transfer.job.id.clone(),
          },
        )
        .await?;
        if !transfer.committed {
          tokio::time::sleep(Duration::from_millis(20)).await;
        }
      }
      let job = super::finish(client, id, check, transfer.job).await?;
      ensure!(
        job.exit_code.is_some_and(|code| code != 0) && job.stdout.is_empty(),
        "Corrupt upload reported successful publication"
      );
    }
    let absent = super::run(
      client,
      id,
      check,
      Action::Start {
        command: Command {
          executable: "/usr/bin/test".into(),
          args: vec!["!".into(), "-e".into(), path],
          directory: None,
          timeout_seconds: 5,
          role: Role::Normal,
        },
      },
    )
    .await?;
    ensure!(
      absent.exit_code == Some(0),
      "Rejected upload left a destination file"
    );
  }
  Ok(())
}
