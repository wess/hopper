use super::{Connection, Phase, Transport};
use anyhow::ensure;
use model::guest::wire::{Message, CHUNK};
use std::time::{Duration, Instant};

impl<T: Transport> Connection<T> {
  pub(super) fn drive(&mut self, now: Instant) -> anyhow::Result<()> {
    ensure!(
      self.phase != Phase::Closed,
      "Guest command connection is closed"
    );
    self.transport.as_ref().unwrap().check()?;
    if self.phase != Phase::Ready {
      ensure!(now < self.deadline, "Guest command transport timed out");
    }
    match self.phase {
      Phase::Waiting => {
        if self.transport.as_ref().unwrap().connected()? {
          self.outgoing = self.handshake.as_ref().unwrap().hello().to_vec();
          self.phase = Phase::Hello;
          self.deadline = now + Duration::from_secs(10);
        }
      }
      Phase::Hello | Phase::Confirm | Phase::Send => self.write()?,
      Phase::Answer => {
        if let Some(count) = self
          .transport
          .as_ref()
          .unwrap()
          .read(&mut self.answer[self.received..])?
        {
          ensure!(
            count != 0 && count <= self.answer.len() - self.received,
            "Guest connection closed or returned invalid authentication bytes"
          );
          self.received += count;
          if self.received == self.answer.len() {
            let (session, confirmation) = self.handshake.take().unwrap().finish(&self.answer)?;
            self.session = Some(session);
            self.outgoing = confirmation.to_vec();
            self.written = 0;
            self.answer.fill(0);
            self.phase = Phase::Confirm;
          }
        }
      }
      Phase::Receive => {
        let mut bytes = [0; CHUNK];
        if let Some(count) = self.transport.as_ref().unwrap().read(&mut bytes)? {
          ensure!(
            count != 0 && count <= bytes.len(),
            "Guest connection closed or exceeded read bounds"
          );
          if let Some(message) = self.session.as_mut().unwrap().feed(&bytes[..count])? {
            let Message::Reply(reply) = message else {
              anyhow::bail!("Guest sent a request to its host");
            };
            ensure!(
              reply.sequence == self.sequence,
              "Guest reply sequence does not match"
            );
            self.sequence = self
              .sequence
              .checked_add(1)
              .ok_or_else(|| anyhow::anyhow!("Guest sequence exhausted"))?;
            self.reply = Some(reply.outcome);
            self.phase = Phase::Ready;
          }
        }
      }
      Phase::Ready => {
        let mut byte = [0; 1];
        if self.transport.as_ref().unwrap().read(&mut byte)?.is_some() {
          anyhow::bail!("Idle guest connection closed or sent unsolicited data");
        }
      }
      Phase::Closed => {}
    }
    self.transport.as_ref().unwrap().check()
  }

  fn write(&mut self) -> anyhow::Result<()> {
    let end = (self.written + CHUNK).min(self.outgoing.len());
    if let Some(count) = self
      .transport
      .as_ref()
      .unwrap()
      .write(&self.outgoing[self.written..end])?
    {
      ensure!(
        count != 0 && count <= end - self.written,
        "Guest connection closed or exceeded write bounds"
      );
      self.written += count;
      if self.written == self.outgoing.len() {
        self.outgoing = Vec::new();
        self.written = 0;
        self.phase = match self.phase {
          Phase::Hello => Phase::Answer,
          Phase::Confirm => Phase::Ready,
          Phase::Send => Phase::Receive,
          _ => unreachable!(),
        };
      }
    }
    Ok(())
  }
}
