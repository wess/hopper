#![cfg(unix)]

use engine::machines::native::sessions::remote::jobs::read_packet;
use model::{
  guest::{
    wire::{Action, Direction, Message, Outcome, Reply, Request},
    Command, Role,
  },
  GuestOs,
};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn guest_job_packets_cross_bounded_transport_chunks_without_shell_conversion() {
  let command = Command {
    executable: "printf".into(),
    args: vec!["\n".repeat(256); 128],
    directory: None,
    timeout_seconds: 10,
    role: Role::Normal,
  };
  let bytes = Request {
    sequence: 1,
    action: Action::Start {
      command: command.clone(),
    },
  }
  .encode(GuestOs::Linux)
  .unwrap();
  assert!(bytes.len() > model::guest::wire::CHUNK);
  let length = bytes.len();
  let (mut reader, mut writer) = tokio::io::duplex(128);
  let task = tokio::spawn(async move {
    writer.write_all(&bytes).await.unwrap();
  });
  let Message::Request(request) = read_packet(&mut reader, length, Direction::Request)
    .await
    .unwrap()
  else {
    panic!("wrong packet");
  };
  let Action::Start { command: received } = request.action else {
    panic!("wrong action");
  };
  assert_eq!(received.args, command.args);
  assert_eq!(received.executable, "printf");
  task.await.unwrap();
  let bytes = Reply {
    sequence: 1,
    outcome: Outcome::Error {
      message: "bounded guest failure".into(),
    },
  }
  .encode()
  .unwrap();
  let Message::Reply(reply) = read_packet(&mut bytes.as_slice(), bytes.len(), Direction::Reply)
    .await
    .unwrap()
  else {
    panic!("wrong reply");
  };
  assert_eq!(reply.sequence, 1);
}

#[tokio::test]
async fn oversized_incomplete_pipelined_and_wrong_direction_guest_packets_reject() {
  for length in [0, 8, (256 << 10) + 9, usize::MAX] {
    assert!(read_packet(&mut b"".as_slice(), length, Direction::Request)
      .await
      .is_err());
  }
  let bytes = Request {
    sequence: 1,
    action: Action::Poll {
      job: "4ae3943e-9d15-44f2-b0f5-5729bc464c0f".into(),
    },
  }
  .encode(GuestOs::Linux)
  .unwrap();
  assert!(read_packet(
    &mut bytes[..bytes.len() - 1].as_ref(),
    bytes.len(),
    Direction::Request
  )
  .await
  .is_err());
  assert!(
    read_packet(&mut bytes.as_slice(), bytes.len(), Direction::Reply)
      .await
      .is_err()
  );
  let mut pipelined = bytes.clone();
  pipelined.extend_from_slice(&bytes);
  assert!(read_packet(
    &mut pipelined.as_slice(),
    pipelined.len(),
    Direction::Request
  )
  .await
  .is_err());
}

#[tokio::test]
async fn file_payloads_cross_multiple_chunks_as_typed_guest_bytes() {
  let file = model::guest::files::Create {
    path: "/home/hopper/new file".into(),
    bytes: vec![255; model::guest::files::CREATE_LIMIT],
    role: Role::Normal,
  };
  let bytes = Request {
    sequence: 1,
    action: Action::CreateFile { file: file.clone() },
  }
  .encode(GuestOs::Linux)
  .unwrap();
  assert!(bytes.len() > model::guest::wire::CHUNK);
  let length = bytes.len();
  let (mut reader, mut writer) = tokio::io::duplex(128);
  let task = tokio::spawn(async move {
    writer.write_all(&bytes).await.unwrap();
  });
  let Message::Request(request) = read_packet(&mut reader, length, Direction::Request)
    .await
    .unwrap()
  else {
    panic!("wrong packet")
  };
  let Action::CreateFile { file: received } = request.action else {
    panic!("wrong action")
  };
  assert_eq!(received.path, file.path);
  assert_eq!(received.bytes, file.bytes);
  assert_eq!(received.role, Role::Normal);
  task.await.unwrap();
}

#[tokio::test]
async fn an_access_toggle_cannot_refresh_the_original_socket_or_queue_policy() {
  let root = tempfile::tempdir().unwrap();
  let manager = engine::machines::Machines {
    root: root.path().join("machines"),
  };
  let id = model::new_uuid();
  let record = model::Machine {
    id: id.clone(),
    name: "Policy fixture".into(),
    guest: GuestOs::Linux,
    profile: "ubuntu".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 2,
      disk_gib: 64,
    },
    installer: None,
    agent_access: true,
    agent_generation: 0,
    runtime: Some(model::MachineRuntime::Virtualization),
  };
  store::json::write(
    &manager.root.join("records").join(format!("{id}.json")),
    &record,
  )
  .unwrap();
  manager.set_agent_access(&id, false).unwrap();
  manager.set_agent_access(&id, true).unwrap();
  let action = Action::Poll {
    job: "4ae3943e-9d15-44f2-b0f5-5729bc464c0f".into(),
  };
  let error = engine::machines::native::sessions::remote::jobs::request_scoped(
    &manager,
    &id,
    0,
    action.clone(),
  )
  .await
  .unwrap_err();
  assert!(error.to_string().contains("Original"));
  assert!(!manager.root.join("agents").exists());
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  {
    let (client, owner) = engine::machines::vz::channel();
    let service = engine::machines::vz::Service::new(manager, client);
    assert!(service
      .guest_request_scoped(&id, 0, action)
      .await
      .unwrap_err()
      .to_string()
      .contains("Original"));
    assert!(!owner.active());
  }
}

#[tokio::test]
async fn maximum_upload_chunk_crosses_transport_boundaries_with_its_offset_intact() {
  let job = model::new_uuid();
  let offset = model::guest::files::UPLOAD_LIMIT - model::guest::files::CREATE_LIMIT as u64;
  let data = vec![255; model::guest::files::CREATE_LIMIT];
  let packet = Request {
    sequence: 1,
    action: Action::UploadChunk {
      job: job.clone(),
      offset,
      bytes: data.clone(),
    },
  }
  .encode(GuestOs::Linux)
  .unwrap();
  assert!(packet.len() > model::guest::wire::CHUNK);
  let length = packet.len();
  let (mut reader, mut writer) = tokio::io::duplex(128);
  let task = tokio::spawn(async move {
    writer.write_all(&packet).await.unwrap();
  });
  let Message::Request(Request {
    action:
      Action::UploadChunk {
        job: actual_job,
        offset: actual_offset,
        bytes,
      },
    ..
  }) = read_packet(&mut reader, length, Direction::Request)
    .await
    .unwrap()
  else {
    panic!("wrong upload request");
  };
  assert_eq!(actual_job, job);
  assert_eq!(actual_offset, offset);
  assert_eq!(bytes, data);
  task.await.unwrap();
}
