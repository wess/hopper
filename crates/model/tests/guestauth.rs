use model::{
  guest::wire::{
    auth::{Boot, GuestHandshake, HostHandshake, Scope, Secret, Session},
    Action, Message, Outcome, Reply, Request, CHUNK,
  },
  guest::{Command, Identity, Job, Phase, Role},
  GuestOs,
};

fn scope() -> Scope {
  Scope {
    boot: Boot {
      machine: "c08b50c4-53a0-459a-8eec-a3d8d4f71f06".into(),
      generation: 1,
      guest: GuestOs::Linux,
    },
    policy: 0,
  }
}

fn key() -> Secret {
  Secret::from_bytes([7; 32]).unwrap()
}

fn pair(scope: Scope) -> (Session, Session) {
  let host = HostHandshake::begin(key(), scope.clone()).unwrap();
  let (guest, answer) = GuestHandshake::answer(key(), &scope.boot, &host.hello()).unwrap();
  let (host, confirmation) = host.finish(&answer).unwrap();
  (host, guest.finish(&confirmation).unwrap())
}

fn request(sequence: u64) -> Message {
  Message::Request(Request {
    sequence,
    action: Action::Poll {
      job: "509a68b2-a8bb-4368-91d1-b080ea17387d".into(),
    },
  })
}

fn reply(sequence: u64) -> Message {
  Message::Reply(Reply {
    sequence,
    outcome: Outcome::Error {
      message: "Guest job is unavailable".into(),
    },
  })
}

#[test]
fn mutually_authenticated_sessions_exchange_only_matching_serial_requests_and_replies() {
  let (mut host, mut guest) = pair(scope());
  assert_eq!(host.scope(), &scope());
  assert_eq!(guest.scope(), &scope());
  for sequence in 1..=3 {
    let frame = host.send(&request(sequence)).unwrap();
    let Message::Request(received) = guest.receive(&frame).unwrap() else {
      panic!("Wrong direction")
    };
    assert_eq!(received.sequence, sequence);
    let frame = guest.send(&reply(sequence)).unwrap();
    let Message::Reply(received) = host.receive(&frame).unwrap() else {
      panic!("Wrong direction")
    };
    assert_eq!(received.sequence, sequence);
  }
}

#[test]
fn altered_handshakes_wrong_keys_and_foreign_boots_reject() {
  let host = HostHandshake::begin(key(), scope()).unwrap();
  let hello = host.hello();
  for index in [0, 4, 20, 28, 29, 37, 69, 100] {
    let mut forged = hello;
    forged[index] ^= 1;
    assert!(GuestHandshake::answer(key(), &scope().boot, &forged).is_err());
  }
  assert!(
    GuestHandshake::answer(Secret::from_bytes([8; 32]).unwrap(), &scope().boot, &hello).is_err()
  );
  let mut foreign = scope().boot;
  foreign.machine = model::new_uuid();
  assert!(GuestHandshake::answer(key(), &foreign, &hello).is_err());
  foreign = scope().boot;
  foreign.generation += 1;
  assert!(GuestHandshake::answer(key(), &foreign, &hello).is_err());
  foreign = scope().boot;
  foreign.guest = GuestOs::Windows;
  assert!(GuestHandshake::answer(key(), &foreign, &hello).is_err());
  for length in [0, 4, 69, 100] {
    assert!(GuestHandshake::answer(key(), &scope().boot, &hello[..length]).is_err());
  }
  let mut extra = hello.to_vec();
  extra.push(0);
  assert!(GuestHandshake::answer(key(), &scope().boot, &extra).is_err());
}

#[test]
fn old_answers_and_confirmations_cannot_authenticate_new_connections() {
  let first = HostHandshake::begin(key(), scope()).unwrap();
  let (first_guest, answer) = GuestHandshake::answer(key(), &scope().boot, &first.hello()).unwrap();
  let (_, confirmation) = first.finish(&answer).unwrap();
  let second = HostHandshake::begin(key(), scope()).unwrap();
  assert!(second.finish(&answer).is_err());
  let old_hello = HostHandshake::begin(key(), scope()).unwrap();
  let (guest, answer) = GuestHandshake::answer(key(), &scope().boot, &old_hello.hello()).unwrap();
  let (_, second_confirmation) = old_hello.finish(&answer).unwrap();
  assert!(guest.finish(&confirmation).is_err());
  assert!(first_guest.finish(&second_confirmation).is_err());
}

#[test]
fn tampered_frames_close_the_session_and_release_its_keys() {
  for index in [0, 4, 8, 20] {
    let (mut host, mut guest) = pair(scope());
    let mut frame = host.send(&request(1)).unwrap();
    let original = frame.clone();
    frame[index] ^= 1;
    assert!(guest.receive(&frame).is_err());
    assert!(guest.receive(&original).is_err());
    assert!(guest.send(&reply(1)).is_err());
  }
  let (mut host, mut guest) = pair(scope());
  let mut frame = host.send(&request(1)).unwrap();
  let end = frame.len() - 1;
  frame[end] ^= 1;
  assert!(guest.receive(&frame).is_err());
}

#[test]
fn replay_pipelining_reflection_and_wrong_reply_sequences_close_sessions() {
  let (mut host, mut guest) = pair(scope());
  let first = host.send(&request(1)).unwrap();
  guest.receive(&first).unwrap();
  let response = guest.send(&reply(1)).unwrap();
  host.receive(&response).unwrap();
  assert!(guest.receive(&first).is_err());
  let (mut host, _) = pair(scope());
  host.send(&request(1)).unwrap();
  assert!(host.send(&request(2)).is_err());
  let (mut host, mut guest) = pair(scope());
  let first = host.send(&request(1)).unwrap();
  guest.receive(&first).unwrap();
  assert!(guest.send(&reply(2)).is_err());
  let (mut host, _) = pair(scope());
  let first = host.send(&request(1)).unwrap();
  assert!(host.receive(&first).is_err());
  let (_, mut guest) = pair(scope());
  assert!(guest.send(&reply(1)).is_err());
  let (mut host, _) = pair(scope());
  assert!(host.send(&request(0)).is_err());
  let (mut host, _) = pair(scope());
  assert!(host.send(&request(u64::MAX)).is_err());
}

#[test]
fn frames_do_not_cross_connections_boots_or_policy_generations() {
  let (mut host, _) = pair(scope());
  let frame = host.send(&request(1)).unwrap();
  let (_, mut other) = pair(scope());
  assert!(other.receive(&frame).is_err());
  let mut changed = scope();
  changed.policy += 1;
  let (_, mut other) = pair(changed);
  assert!(other.receive(&frame).is_err());
  let mut changed = scope();
  changed.boot.generation += 1;
  let (_, mut other) = pair(changed);
  assert!(other.receive(&frame).is_err());
}

#[test]
fn zero_keys_invalid_scopes_and_truncated_confirmation_reject() {
  assert!(Secret::from_bytes([0; 32]).is_err());
  let mut invalid = scope();
  invalid.boot.generation = 0;
  assert!(HostHandshake::begin(key(), invalid).is_err());
  let mut invalid = scope();
  invalid.boot.machine = "00000000-0000-0000-0000-000000000000".into();
  assert!(HostHandshake::begin(key(), invalid).is_err());
  let host = HostHandshake::begin(key(), scope()).unwrap();
  let (guest, answer) = GuestHandshake::answer(key(), &scope().boot, &host.hello()).unwrap();
  let (_, confirmation) = host.finish(&answer).unwrap();
  assert!(guest.finish(&confirmation[..35]).is_err());
}

#[test]
fn authenticated_frames_survive_every_fragment_boundary_and_reject_extra_bytes() {
  let (mut host, _) = pair(scope());
  let length = host.send(&request(1)).unwrap().len();
  for split in 0..length {
    let (mut host, mut guest) = pair(scope());
    let frame = host.send(&request(1)).unwrap();
    assert!(guest.feed(&frame[..split]).unwrap().is_none());
    assert!(matches!(
      guest.feed(&frame[split..]).unwrap(),
      Some(Message::Request(_))
    ));
    let response = guest.send(&reply(1)).unwrap();
    let mut received = None;
    for byte in response.chunks(1) {
      if let Some(reply) = host.feed(byte).unwrap() {
        received = Some(reply);
      }
    }
    assert!(matches!(received, Some(Message::Reply(_))));
  }
  let (mut host, mut guest) = pair(scope());
  let frame = host.send(&request(1)).unwrap();
  assert!(guest
    .feed(&[frame.clone(), frame.clone()].concat())
    .is_err());
  assert!(guest.feed(&frame).is_err());
  let (_, mut guest) = pair(scope());
  assert!(guest.feed(&vec![0; CHUNK + 1]).is_err());
  let (_, mut guest) = pair(scope());
  assert!(guest.feed(b"HPG1\0\x04\0\x01").is_err());
}

#[test]
fn authenticated_large_requests_and_full_binary_output_use_bounded_io_chunks() {
  let (mut host, mut guest) = pair(scope());
  let request = Message::Request(Request {
    sequence: 1,
    action: Action::Start {
      command: Command {
        executable: "/bin/echo".into(),
        args: vec!["\u{1}".repeat(4096); 8],
        directory: None,
        role: Role::Normal,
        timeout_seconds: 5,
      },
    },
  });
  let frame = host.send(&request).unwrap();
  assert!(frame.len() > CHUNK);
  let mut received = None;
  for chunk in frame.chunks(CHUNK) {
    if let Some(message) = guest.feed(chunk).unwrap() {
      received = Some(message);
    }
  }
  assert!(matches!(received, Some(Message::Request(_))));
  let frame = guest
    .send(&Message::Reply(Reply {
      sequence: 1,
      outcome: Outcome::Job {
        job: Job {
          id: model::new_uuid(),
          phase: Phase::Running,
          finished: false,
          identity: Some(Identity {
            user: "hopper".into(),
            uid: 1001,
            gid: 1001,
            groups: vec![1001],
            administrator: false,
          }),
          exit_code: None,
          signal: None,
          stdout: vec![255; CHUNK],
          stderr: vec![255; CHUNK],
          stdout_truncated: true,
          stderr_truncated: true,
          error: None,
          cleanup_error: None,
        },
      },
    }))
    .unwrap();
  let mut received = None;
  for chunk in frame.chunks(CHUNK) {
    if let Some(message) = host.feed(chunk).unwrap() {
      received = Some(message);
    }
  }
  let Some(Message::Reply(Reply {
    outcome: Outcome::Job { job },
    ..
  })) = received
  else {
    panic!("Missing authenticated output");
  };
  assert_eq!(job.stdout, vec![255; CHUNK]);
  assert_eq!(job.stderr, vec![255; CHUNK]);
}
