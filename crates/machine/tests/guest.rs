use machine::guest::{self, Connection, Transport};
use model::{
  guest::{
    wire::{
      auth::{Boot, GuestHandshake, Material, Secret, Session},
      Action, Message, Outcome, Reply, Request,
    },
    Command, Role,
  },
  GuestOs,
};
use std::{
  cell::{Cell, RefCell},
  collections::VecDeque,
  rc::Rc,
  time::{Duration, Instant},
};

struct Peer {
  boot: Boot,
  bytes: Vec<u8>,
  answer: VecDeque<u8>,
  pending: Option<GuestHandshake>,
  session: Option<Session>,
  request: Option<Request>,
}

struct State {
  allowed: Cell<bool>,
  connected: Cell<bool>,
  closed: Cell<bool>,
  blocked: Cell<bool>,
  revoke_read: Cell<bool>,
  eof: Cell<bool>,
  corrupt: Cell<bool>,
  checks: Cell<usize>,
  revoke_check: Cell<Option<usize>>,
  peer: RefCell<Peer>,
}

struct Mock(Rc<State>);

impl std::ops::Deref for Mock {
  type Target = State;

  fn deref(&self) -> &State {
    &self.0
  }
}

impl Transport for Mock {
  fn check(&self) -> anyhow::Result<()> {
    self.checks.set(self.checks.get() + 1);
    if self.revoke_check.get() == Some(self.checks.get()) {
      self.allowed.set(false);
    }
    anyhow::ensure!(self.allowed.get(), "Original agent policy was revoked");
    Ok(())
  }

  fn connected(&self) -> anyhow::Result<bool> {
    Ok(self.connected.get() && !self.closed.get())
  }

  fn read(&self, bytes: &mut [u8]) -> anyhow::Result<Option<usize>> {
    if self.eof.get() {
      return Ok(Some(0));
    }
    let mut peer = self.peer.borrow_mut();
    if peer.answer.is_empty() {
      return Ok(None);
    }
    let count = bytes.len().min(peer.answer.len()).min(5);
    for byte in &mut bytes[..count] {
      *byte = peer.answer.pop_front().unwrap();
    }
    if self.revoke_read.get() {
      self.allowed.set(false);
    }
    Ok(Some(count))
  }

  fn write(&self, bytes: &[u8]) -> anyhow::Result<Option<usize>> {
    if self.blocked.get() {
      return Ok(None);
    }
    let count = bytes.len().min(7);
    let mut peer = self.peer.borrow_mut();
    if let Some(session) = &mut peer.session {
      if let Some(Message::Request(request)) = session.feed(&bytes[..count])? {
        let mut reply = session.send(&Message::Reply(Reply {
          sequence: request.sequence,
          outcome: Outcome::Error {
            message: "Guest fixture result".into(),
          },
        }))?;
        if self.corrupt.get() {
          let end = reply.len() - 1;
          reply[end] ^= 1;
        }
        peer.request = Some(request);
        peer.answer.extend(reply);
      }
    } else {
      peer.bytes.extend_from_slice(&bytes[..count]);
      if peer.pending.is_none() && peer.bytes.len() == 101 {
        let (pending, answer) =
          GuestHandshake::answer(Secret::from_bytes([13; 32])?, &peer.boot, &peer.bytes)?;
        peer.pending = Some(pending);
        peer.answer.extend(answer);
        peer.bytes.clear();
      } else if peer.pending.is_some() && peer.bytes.len() == 36 {
        peer.session = Some(peer.pending.take().unwrap().finish(&peer.bytes)?);
        peer.bytes.clear();
      }
    }
    Ok(Some(count))
  }

  fn disconnect(&self) {
    self.closed.set(true);
  }
}

fn fixture() -> (Rc<State>, Material) {
  let boot = Boot {
    machine: "9d40bfe7-6872-4ccd-af9b-4ba6f48a0202".into(),
    generation: 2,
    guest: GuestOs::Linux,
  };
  let state = Rc::new(State {
    allowed: Cell::new(true),
    connected: Cell::new(true),
    closed: Cell::new(false),
    blocked: Cell::new(false),
    revoke_read: Cell::new(false),
    eof: Cell::new(false),
    corrupt: Cell::new(false),
    checks: Cell::new(0),
    revoke_check: Cell::new(None),
    peer: RefCell::new(Peer {
      boot: boot.clone(),
      bytes: Vec::new(),
      answer: VecDeque::new(),
      pending: None,
      session: None,
      request: None,
    }),
  });
  (
    state,
    Material {
      boot,
      key: Secret::from_bytes([13; 32]).unwrap(),
    },
  )
}

fn command() -> Action {
  Action::Start {
    command: Command {
      executable: "/bin/echo".into(),
      args: vec!["literal; id".into()],
      directory: None,
      role: Role::Normal,
      timeout_seconds: 60,
    },
  }
}

fn ready(connection: &mut Connection<Mock>, now: Instant) {
  for _ in 0..100 {
    connection.tick(now).unwrap();
    if connection.ready() {
      return;
    }
  }
  panic!("Connection did not authenticate");
}

fn reply(connection: &mut Connection<Mock>, now: Instant) -> Outcome {
  for _ in 0..200 {
    connection.tick(now).unwrap();
    if let Some(reply) = connection.take_reply().unwrap() {
      return reply;
    }
  }
  panic!("Connection did not return a reply");
}

#[test]
fn fragmented_handshake_and_serial_commands_round_trip_without_host_execution() {
  let now = Instant::now();
  let (state, material) = fixture();
  let mut connection = guest::connect(Mock(state.clone()), material, 7, now).unwrap();
  ready(&mut connection, now);
  for sequence in 1..=2 {
    connection.request(command(), now).unwrap();
    assert!(connection.request(command(), now).is_err());
    assert!(
      matches!(reply(&mut connection, now), Outcome::Error { message } if message == "Guest fixture result")
    );
    let peer = state.peer.borrow();
    let received = peer.request.as_ref().unwrap();
    assert_eq!(received.sequence, sequence);
    assert_eq!(peer.session.as_ref().unwrap().scope().policy, 7);
    let Action::Start { command } = &received.action else {
      panic!("Wrong action")
    };
    assert_eq!(command.args, ["literal; id"]);
    assert_eq!(command.role, Role::Normal);
  }
  drop(connection);
  assert!(state.closed.get());
}

#[test]
fn waiting_authentication_and_request_timeouts_disconnect() {
  let now = Instant::now();
  let (state, material) = fixture();
  state.connected.set(false);
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  assert!(connection.tick(now + Duration::from_secs(180)).is_err());
  assert!(state.closed.get());
  let (state, material) = fixture();
  state.blocked.set(true);
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  connection.tick(now).unwrap();
  assert!(connection.tick(now + Duration::from_secs(10)).is_err());
  assert!(state.closed.get());
  let (state, material) = fixture();
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  ready(&mut connection, now);
  connection.request(command(), now).unwrap();
  state.blocked.set(true);
  assert!(connection.tick(now + Duration::from_secs(5)).is_err());
  assert!(state.closed.get());
}

#[test]
fn revocation_during_read_and_before_reply_publication_discards_data() {
  let now = Instant::now();
  let (state, material) = fixture();
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  ready(&mut connection, now);
  connection.request(command(), now).unwrap();
  state.revoke_read.set(true);
  let mut failed = false;
  for _ in 0..200 {
    if connection.tick(now).is_err() {
      failed = true;
      break;
    }
  }
  assert!(failed && state.closed.get());
  assert!(connection.take_reply().is_err());
  let (state, material) = fixture();
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  ready(&mut connection, now);
  connection.request(command(), now).unwrap();
  for _ in 0..200 {
    connection.tick(now).unwrap();
  }
  state.revoke_check.set(Some(state.checks.get() + 2));
  assert!(connection.take_reply().is_err());
  assert!(state.closed.get());
}

#[test]
fn invalid_commands_keep_a_ready_connection_and_constructor_failure_closes_it() {
  let now = Instant::now();
  let (state, material) = fixture();
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  ready(&mut connection, now);
  assert!(connection
    .request(
      Action::Cancel {
        job: "../host".into()
      },
      now
    )
    .is_err());
  assert!(connection.ready() && !state.closed.get());
  let (state, material) = fixture();
  state.allowed.set(false);
  assert!(guest::connect(Mock(state.clone()), material, 0, now).is_err());
  assert!(state.closed.get());
}

#[test]
fn eof_corrupt_replies_and_explicit_close_drop_the_transport_owner() {
  let now = Instant::now();
  for corrupt in [false, true] {
    let (state, material) = fixture();
    let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
    ready(&mut connection, now);
    state.corrupt.set(corrupt);
    state.eof.set(!corrupt);
    connection.request(command(), now).unwrap();
    let mut failed = false;
    for _ in 0..200 {
      if connection.tick(now).is_err() {
        failed = true;
        break;
      }
    }
    assert!(failed && state.closed.get());
    assert_eq!(Rc::strong_count(&state), 1);
    assert!(connection.take_reply().is_err());
    assert!(connection.request(command(), now).is_err());
  }
  let (state, material) = fixture();
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  assert_eq!(Rc::strong_count(&state), 2);
  connection.close();
  assert_eq!(Rc::strong_count(&state), 1);
  let (state, material) = fixture();
  let mut connection = guest::connect(Mock(state.clone()), material, 0, now).unwrap();
  ready(&mut connection, now);
  state.eof.set(true);
  assert!(connection.tick(now).is_err());
  assert_eq!(Rc::strong_count(&state), 1);
}
