use super::{frames::Frames, Scope, Secret};
use crate::guest::wire::{require, Decoder, Direction, Message, CHUNK};
use std::io;

pub struct Session {
  scope: Scope,
  send: Option<Secret>,
  receive: Option<Secret>,
  host: bool,
  sequence: u64,
  waiting: bool,
  frames: Frames,
}

impl Session {
  pub(super) fn new(key: &Secret, scope: Scope, transcript: &[u8], host: bool) -> Self {
    let request = Secret(key.sign(b"request-key", transcript));
    let reply = Secret(key.sign(b"reply-key", transcript));
    let (send, receive) = if host {
      (request, reply)
    } else {
      (reply, request)
    };
    Self {
      scope,
      send: Some(send),
      receive: Some(receive),
      host,
      sequence: 1,
      waiting: false,
      frames: Frames::default(),
    }
  }

  pub fn scope(&self) -> &Scope {
    &self.scope
  }

  pub fn is_host(&self) -> bool {
    self.host
  }

  pub fn send(&mut self, message: &Message) -> io::Result<Vec<u8>> {
    let result = self.encode(message);
    if result.is_err() {
      self.close();
    }
    result
  }

  fn encode(&mut self, message: &Message) -> io::Result<Vec<u8>> {
    require(self.send.is_some() && self.sequence != u64::MAX)?;
    let (mut frame, sequence) = match message {
      Message::Request(request) if self.host && !self.waiting => {
        (request.encode(self.scope.boot.guest)?, request.sequence)
      }
      Message::Reply(reply) if !self.host && self.waiting => (reply.encode()?, reply.sequence),
      _ => {
        return Err(io::Error::new(
          io::ErrorKind::InvalidData,
          "Unexpected guest message",
        ))
      }
    };
    require(sequence == self.sequence)?;
    let proof = self.send.as_ref().unwrap().sign(b"frame", &frame);
    frame.extend_from_slice(&proof);
    self.waiting = self.host;
    if !self.host {
      self.sequence += 1;
    }
    Ok(frame)
  }

  pub fn receive(&mut self, frame: &[u8]) -> io::Result<Message> {
    let result = require(self.frames.empty()).and_then(|()| self.decode(frame));
    if result.is_err() {
      self.close();
    }
    result
  }

  pub fn feed(&mut self, bytes: &[u8]) -> io::Result<Option<Message>> {
    let result = self.append(bytes);
    if result.is_err() {
      self.close();
    }
    result
  }

  fn append(&mut self, bytes: &[u8]) -> io::Result<Option<Message>> {
    require(self.receive.is_some() && self.waiting == self.host)?;
    let direction = if self.host {
      Direction::Reply
    } else {
      Direction::Request
    };
    match self.frames.feed(bytes, direction)? {
      Some(frame) => self.decode(&frame).map(Some),
      None => Ok(None),
    }
  }

  fn decode(&mut self, frame: &[u8]) -> io::Result<Message> {
    require(self.receive.is_some() && self.sequence != u64::MAX)?;
    let direction = if self.host {
      Direction::Reply
    } else {
      Direction::Request
    };
    require((41..=direction.limit() + 40).contains(&frame.len()))?;
    let end = frame.len() - 32;
    self
      .receive
      .as_ref()
      .unwrap()
      .verify(b"frame", &frame[..end], &frame[end..])?;
    let mut decoder = Decoder::new(direction, self.scope.boot.guest);
    for chunk in frame[..end].chunks(CHUNK) {
      decoder.feed(chunk)?;
    }
    let message = decoder.take()?.ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "Incomplete authenticated guest message",
      )
    })?;
    let sequence = match &message {
      Message::Request(request) if !self.host && !self.waiting => request.sequence,
      Message::Reply(reply) if self.host && self.waiting => reply.sequence,
      _ => {
        return Err(io::Error::new(
          io::ErrorKind::InvalidData,
          "Unexpected guest message",
        ))
      }
    };
    require(sequence == self.sequence)?;
    self.waiting = !self.host;
    if self.host {
      self.sequence += 1;
    }
    Ok(message)
  }

  pub fn close(&mut self) {
    self.send = None;
    self.receive = None;
    self.waiting = false;
    self.frames = Frames::default();
  }
}
