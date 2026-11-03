mod drive;

use anyhow::ensure;
use model::guest::wire::{
  auth::{HostHandshake, Material, Session},
  Action, Message, Outcome, Request,
};
use std::time::{Duration, Instant};

/// implementations must be nonblocking and check the original VM/policy generation.
pub trait Transport {
  fn check(&self) -> anyhow::Result<()>;
  fn connected(&self) -> anyhow::Result<bool>;
  fn read(&self, bytes: &mut [u8]) -> anyhow::Result<Option<usize>>;
  fn write(&self, bytes: &[u8]) -> anyhow::Result<Option<usize>>;
  fn disconnect(&self);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
  Waiting,
  Hello,
  Answer,
  Confirm,
  Ready,
  Send,
  Receive,
  Closed,
}

pub struct Connection<T: Transport> {
  transport: Option<T>,
  phase: Phase,
  handshake: Option<HostHandshake>,
  session: Option<Session>,
  outgoing: Vec<u8>,
  written: usize,
  answer: [u8; 68],
  received: usize,
  deadline: Instant,
  sequence: u64,
  reply: Option<Outcome>,
}

pub fn connect<T: Transport>(
  transport: T,
  material: Material,
  policy: u64,
  now: Instant,
) -> anyhow::Result<Connection<T>> {
  connect_borrowed(transport, &material, policy, now)
}

pub fn connect_borrowed<T: Transport>(
  transport: T,
  material: &Material,
  policy: u64,
  now: Instant,
) -> anyhow::Result<Connection<T>> {
  let result = (|| -> anyhow::Result<HostHandshake> {
    transport.check()?;
    let handshake = material.handshake(policy)?;
    transport.check()?;
    Ok(handshake)
  })();
  let handshake = match result {
    Ok(handshake) => handshake,
    Err(error) => {
      transport.disconnect();
      return Err(error);
    }
  };
  Ok(Connection {
    transport: Some(transport),
    phase: Phase::Waiting,
    handshake: Some(handshake),
    session: None,
    outgoing: Vec::new(),
    written: 0,
    answer: [0; 68],
    received: 0,
    deadline: now + Duration::from_secs(180),
    sequence: 1,
    reply: None,
  })
}

impl<T: Transport> Connection<T> {
  pub fn ready(&self) -> bool {
    self.phase == Phase::Ready && self.reply.is_none()
  }

  pub fn tick(&mut self, now: Instant) -> anyhow::Result<()> {
    let result = self.drive(now);
    if result.is_err() {
      self.close();
    }
    result
  }

  pub fn request(&mut self, action: Action, now: Instant) -> anyhow::Result<()> {
    ensure!(
      self.phase != Phase::Closed,
      "Guest command connection is closed"
    );
    if let Err(error) = self.transport.as_ref().unwrap().check() {
      self.close();
      return Err(error);
    }
    ensure!(
      self.ready(),
      "Guest command connection is not ready or is busy"
    );
    let request = Request {
      sequence: self.sequence,
      action,
    };
    let session = self.session.as_mut().unwrap();
    ensure!(
      request.valid_for(session.scope().boot.guest),
      "Guest command has invalid fields"
    );
    let result = (|| -> anyhow::Result<()> {
      self.outgoing = session.send(&Message::Request(request))?;
      self.transport.as_ref().unwrap().check()?;
      self.written = 0;
      self.deadline = now + Duration::from_secs(5);
      self.phase = Phase::Send;
      Ok(())
    })();
    if result.is_err() {
      self.close();
    }
    result
  }

  pub fn take_reply(&mut self) -> anyhow::Result<Option<Outcome>> {
    ensure!(
      self.phase != Phase::Closed,
      "Guest command connection is closed"
    );
    if let Err(error) = self.transport.as_ref().unwrap().check() {
      self.close();
      return Err(error);
    }
    let reply = self.reply.take();
    if let Err(error) = self.transport.as_ref().unwrap().check() {
      self.close();
      return Err(error);
    }
    Ok(reply)
  }

  pub fn close(&mut self) {
    self.phase = Phase::Closed;
    if let Some(transport) = self.transport.take() {
      transport.disconnect();
    }
    self.handshake = None;
    self.session = None;
    self.outgoing = Vec::new();
    self.answer.fill(0);
    self.reply = None;
  }
}

impl<T: Transport> Drop for Connection<T> {
  fn drop(&mut self) {
    self.close();
  }
}
