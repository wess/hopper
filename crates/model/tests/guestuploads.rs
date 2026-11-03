use model::{
  guest::{
    files::{Transfer, Upload, CREATE_LIMIT, UPLOAD_LIMIT},
    wire::{Action, Decoder, Direction, Message, Outcome, Reply, Request},
    Job, Phase, Role,
  },
  GuestOs,
};

fn spec() -> Upload {
  Upload {
    path: "/home/hopper/new".into(),
    size: UPLOAD_LIMIT,
    sha256: "ab".repeat(32),
    timeout_seconds: 600,
    role: Role::Normal,
  }
}

#[test]
fn upload_actions_require_linux_and_bounded_literal_metadata() {
  let id = model::new_uuid();
  for action in [
    Action::BeginUpload { file: spec() },
    Action::UploadChunk {
      job: id.clone(),
      offset: UPLOAD_LIMIT - CREATE_LIMIT as u64,
      bytes: vec![255; CREATE_LIMIT],
    },
    Action::UploadStatus { job: id.clone() },
    Action::CommitUpload { job: id.clone() },
  ] {
    let request = Request {
      sequence: 1,
      action,
    };
    let bytes = request.encode(GuestOs::Linux).unwrap();
    let mut decoder = Decoder::new(Direction::Request, GuestOs::Linux);
    for chunk in bytes.chunks(97) {
      decoder.feed(chunk).unwrap();
    }
    assert!(matches!(decoder.take().unwrap(), Some(Message::Request(_))));
    assert!(!request.valid_for(GuestOs::Windows));
    assert!(!request.valid_for(GuestOs::Macos));
  }
  for (offset, bytes) in [
    (0, Vec::new()),
    (0, vec![0; CREATE_LIMIT + 1]),
    (UPLOAD_LIMIT, vec![0]),
    (u64::MAX, vec![0]),
  ] {
    assert!(!Request {
      sequence: 1,
      action: Action::UploadChunk {
        job: id.clone(),
        offset,
        bytes
      }
    }
    .valid_for(GuestOs::Linux));
  }
  for action in [
    Action::UploadStatus {
      job: "../host".into(),
    },
    Action::CommitUpload {
      job: "../host".into(),
    },
  ] {
    assert!(!Request {
      sequence: 1,
      action
    }
    .valid_for(GuestOs::Linux));
  }
}

#[test]
fn transfer_reply_checks_offsets_commit_and_nested_job() {
  let job = Job {
    id: model::new_uuid(),
    identity: None,
    phase: Phase::Starting,
    finished: false,
    exit_code: None,
    signal: None,
    stdout: Vec::new(),
    stderr: Vec::new(),
    stdout_truncated: false,
    stderr_truncated: false,
    error: None,
    cleanup_error: None,
  };
  let mut transfer = Transfer {
    job,
    offset: 0,
    size: 3,
    committed: false,
  };
  let valid = |transfer: &Transfer| {
    Reply {
      sequence: 1,
      outcome: Outcome::Transfer {
        transfer: transfer.clone(),
      },
    }
    .valid()
  };
  assert!(valid(&transfer));
  transfer.committed = true;
  assert!(!valid(&transfer));
  transfer.offset = 3;
  assert!(valid(&transfer));
  let bytes = Reply {
    sequence: 1,
    outcome: Outcome::Transfer {
      transfer: transfer.clone(),
    },
  }
  .encode()
  .unwrap();
  let mut decoder = Decoder::new(Direction::Reply, GuestOs::Linux);
  assert!(decoder.feed(&bytes).unwrap());
  assert!(matches!(
    decoder.take().unwrap(),
    Some(Message::Reply(Reply {
      outcome: Outcome::Transfer { .. },
      ..
    }))
  ));
  transfer.offset = 4;
  assert!(!valid(&transfer));
  transfer.offset = 3;
  transfer.size = UPLOAD_LIMIT + 1;
  assert!(!valid(&transfer));
  transfer.size = 3;
  transfer.job.id = "../host".into();
  assert!(!valid(&transfer));
}
