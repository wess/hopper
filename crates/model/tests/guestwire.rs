use model::{
  guest::{
    wire::{Action, Decoder, Direction, Message, Outcome, Reply, Request, CHUNK},
    Command, Job, Phase, Role,
  },
  GuestOs,
};

fn request() -> Request {
  Request {
    sequence: 1,
    action: Action::Start {
      command: Command {
        executable: "/bin/echo".into(),
        args: vec!["literal; id".into()],
        directory: None,
        timeout_seconds: 60,
        role: Role::Normal,
      },
    },
  }
}

fn frame(bytes: &[u8]) -> Vec<u8> {
  let mut frame = b"HPG1".to_vec();
  frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
  frame.extend_from_slice(bytes);
  frame
}

#[test]
fn every_request_boundary_survives_fragmentation_and_decoder_reuse() {
  let encoded = request().encode(GuestOs::Linux).unwrap();
  for split in 0..=encoded.len() {
    let mut decoder = Decoder::new(Direction::Request, GuestOs::Linux);
    decoder.feed(&encoded[..split]).unwrap();
    assert!(decoder.feed(&encoded[split..]).unwrap());
    let Some(Message::Request(decoded)) = decoder.take().unwrap() else {
      panic!("Missing request");
    };
    assert_eq!(decoded.sequence, 1);
    let Action::Start { command } = decoded.action else {
      panic!("Wrong action");
    };
    assert_eq!(command.args, ["literal; id"]);
    assert!(decoder.take().unwrap().is_none());
    assert!(decoder.feed(&encoded).unwrap());
    assert!(decoder.take().unwrap().is_some());
  }
}

#[test]
fn invalid_headers_and_pipelining_poison_the_connection() {
  let good = request().encode(GuestOs::Linux).unwrap();
  for bad in [
    b"HPG2\0\0\0\x01".to_vec(),
    b"HPG1\0\0\0\0".to_vec(),
    b"HPG1\0\x04\0\x01".to_vec(),
    [good.clone(), good.clone()].concat(),
    vec![0; CHUNK + 1],
  ] {
    let mut decoder = Decoder::new(Direction::Request, GuestOs::Linux);
    assert!(decoder.feed(&bad).is_err());
    assert!(decoder.feed(&good).is_err());
    assert!(decoder.take().is_err());
  }
  let mut decoder = Decoder::new(Direction::Request, GuestOs::Linux);
  assert!(decoder.feed(&good).unwrap());
  assert!(decoder.feed(&[0]).is_err());
}

#[test]
fn unknown_fields_invalid_commands_ids_and_zero_sequences_reject() {
  let valid = serde_json::to_value(request()).unwrap();
  let mut extra = valid.clone();
  extra["uid"] = 0.into();
  let mut action = valid.clone();
  action["action"]["env"] = serde_json::json!({"HOME": "/root"});
  let mut zero = valid.clone();
  zero["sequence"] = 0.into();
  let mut invalid = valid.clone();
  invalid["action"]["command"]["timeoutSeconds"] = 601.into();
  for value in [
    extra,
    action,
    zero,
    invalid,
    serde_json::json!({
      "sequence": 1, "action": {"action": "cancel", "job": "../../../host"}
    }),
  ] {
    let encoded = frame(&serde_json::to_vec(&value).unwrap());
    let mut decoder = Decoder::new(Direction::Request, GuestOs::Linux);
    assert!(decoder.feed(&encoded).unwrap());
    assert!(decoder.take().is_err());
    assert!(decoder
      .feed(&request().encode(GuestOs::Linux).unwrap())
      .is_err());
  }
}

#[test]
fn maximum_escaped_arguments_and_both_output_streams_fit_their_frames() {
  let mut maximum = request();
  let Action::Start { command } = &mut maximum.action else {
    unreachable!()
  };
  command.args = vec!["\u{1}".repeat(4096); 8];
  command.directory = Some(format!("/{}", "\u{1}".repeat(4095)));
  let encoded = maximum.encode(GuestOs::Linux).unwrap();
  let mut decoder = Decoder::new(Direction::Request, GuestOs::Linux);
  for chunk in encoded.chunks(CHUNK) {
    decoder.feed(chunk).unwrap();
  }
  assert!(decoder.take().unwrap().is_some());
  let mut reply = Reply {
    sequence: 2,
    outcome: Outcome::Job {
      job: Job {
        id: model::new_uuid(),
        identity: None,
        phase: Phase::Failed,
        finished: true,
        exit_code: None,
        signal: None,
        stdout: vec![255; 64 << 10],
        stderr: vec![255; 64 << 10],
        stdout_truncated: true,
        stderr_truncated: true,
        error: Some("Failed".into()),
        cleanup_error: None,
      },
    },
  };
  let encoded = reply.encode().unwrap();
  let mut decoder = Decoder::new(Direction::Reply, GuestOs::Linux);
  for chunk in encoded.chunks(CHUNK) {
    decoder.feed(chunk).unwrap();
  }
  let Some(Message::Reply(decoded)) = decoder.take().unwrap() else {
    panic!("Missing reply")
  };
  let Outcome::Job { job } = decoded.outcome else {
    panic!("Missing job")
  };
  assert_eq!(job.stdout, vec![255; 64 << 10]);
  assert_eq!(job.stderr, vec![255; 64 << 10]);
  let Outcome::Job { job } = &mut reply.outcome else {
    unreachable!()
  };
  job.stdout.push(0);
  assert!(reply.encode().is_err());
  let Outcome::Job { job } = &mut reply.outcome else {
    unreachable!()
  };
  job.stdout.pop();
  job.phase = Phase::Completed;
  assert!(
    reply.encode().is_err(),
    "Completed execution must have an identity"
  );
  let Outcome::Job { job } = &mut reply.outcome else {
    unreachable!()
  };
  job.phase = Phase::Starting;
  assert!(reply.encode().is_err(), "Starting jobs cannot be finished");
}

#[test]
fn malformed_json_wrong_direction_and_invalid_replies_reject() {
  for bytes in [b"{}".as_slice(), b"{", b"{\"sequence\":1,\"sequence\":2}"] {
    let mut decoder = Decoder::new(Direction::Request, GuestOs::Linux);
    decoder.feed(&frame(bytes)).unwrap();
    assert!(decoder.take().is_err());
  }
  let mut decoder = Decoder::new(Direction::Reply, GuestOs::Linux);
  decoder
    .feed(&request().encode(GuestOs::Linux).unwrap())
    .unwrap();
  assert!(decoder.take().is_err());
  let reply = Reply {
    sequence: 1,
    outcome: Outcome::Error {
      message: "x".repeat(513),
    },
  };
  assert!(reply.encode().is_err());
  let mut decoder = Decoder::new(Direction::Reply, GuestOs::Linux);
  decoder
    .feed(&frame(&serde_json::to_vec(&reply).unwrap()))
    .unwrap();
  assert!(decoder.take().is_err());
}
