use super::{Check, Client, Owner, Request};
use anyhow::{ensure, Context};
use model::guest::wire::{Action, Outcome};
use std::time::Instant;
use tokio::sync::oneshot;

pub(super) struct Query {
  id: String,
  generation: u64,
  policy: u64,
  action: Action,
  session: Check,
  check: Check,
  send: oneshot::Sender<anyhow::Result<Outcome>>,
}

pub(super) struct Link {
  connection: crate::guest::Connection<super::super::socket::Listener>,
  generation: u64,
  policy: u64,
  pending: Option<Query>,
  sent: bool,
}

impl Client {
  /// the session check must not retain the per-request operation lease.
  pub async fn guest_request(
    &self,
    id: &str,
    generation: u64,
    policy: u64,
    action: Action,
    checks: (Check, Check),
  ) -> anyhow::Result<Outcome> {
    super::validate(id)?;
    ensure!(
      generation != 0,
      "Guest request requires an owned hardware generation"
    );
    ensure!(
      model::guest::wire::Request {
        sequence: 1,
        action: action.clone()
      }
      .valid_for(model::GuestOs::Linux),
      "Invalid Linux guest request"
    );
    let (session, check) = checks;
    check()?;
    session()?;
    let (send, receive) = oneshot::channel();
    self
      .0
      .send(Request::Guest(Query {
        id: id.into(),
        generation,
        policy,
        action,
        session: session.clone(),
        check: check.clone(),
        send,
      }))
      .await
      .context("VZ owner is unavailable")?;
    self.1.notify_one();
    let result = receive
      .await
      .context("Guest request ended without a result")?;
    check()?;
    session()?;
    result
  }
}

impl Owner {
  pub(super) fn guest_request(&mut self, query: Query) {
    if query.send.is_closed() {
      return;
    }
    let result = (|| -> anyhow::Result<()> {
      (query.check)()?;
      (query.session)()?;
      let vm = self
        .machines
        .get(&query.id)
        .context("VZ machine is not owned")?;
      ensure!(
        vm.generation.get() == query.generation,
        "Guest hardware generation changed"
      );
      ensure!(
        !self.pending.contains_key(&query.id) && !self.installations.contains_key(&query.id),
        "VZ operation is active"
      );
      if self
        .jobs
        .get(&query.id)
        .is_some_and(|link| link.generation != query.generation || link.policy != query.policy)
      {
        self.jobs.remove(&query.id);
      }
      if !self.jobs.contains_key(&query.id) {
        ensure!(self.jobs.len() < 16, "Guest connection limit reached");
        let connection = self.commands(&query.id, query.policy, query.session.clone())?;
        self.jobs.insert(
          query.id.clone(),
          Link {
            connection,
            generation: query.generation,
            policy: query.policy,
            pending: None,
            sent: false,
          },
        );
      }
      let link = self.jobs.get(&query.id).unwrap();
      ensure!(link.pending.is_none(), "Another guest request is active");
      Ok(())
    })();
    if let Err(error) = result {
      let _ = query.send.send(Err(error));
      return;
    }
    let link = self.jobs.get_mut(&query.id).unwrap();
    link.pending = Some(query);
    link.sent = false;
  }

  pub(super) fn tick_jobs(&mut self) {
    let now = Instant::now();
    if now < self.jobs_due {
      return;
    }
    self.jobs_due = now + std::time::Duration::from_millis(50);
    let mut closed = Vec::new();
    for (id, link) in &mut self.jobs {
      let result = (|| -> anyhow::Result<()> {
        let vm = self.machines.get(id).context("VZ machine is not owned")?;
        ensure!(
          vm.generation.get() == link.generation,
          "Guest hardware generation changed"
        );
        if let Some(query) = &link.pending {
          ensure!(!query.send.is_closed(), "Guest caller disconnected");
          (query.check)()?;
          (query.session)()?;
        }
        link.connection.tick(now)?;
        if let Some(query) = &link.pending {
          (query.check)()?;
          (query.session)()?;
          if !link.sent && link.connection.ready() {
            link.connection.request(query.action.clone(), now)?;
            link.sent = true;
          }
          if link.sent {
            if let Some(reply) = link.connection.take_reply()? {
              (query.check)()?;
              (query.session)()?;
              let query = link.pending.take().unwrap();
              let _ = query.send.send(Ok(reply));
              link.sent = false;
            }
          }
        }
        Ok(())
      })();
      if let Err(error) = result {
        if let Some(query) = link.pending.take() {
          let _ = query.send.send(Err(error));
        }
        closed.push(id.clone());
      }
    }
    for id in closed {
      self.jobs.remove(&id);
    }
  }
}
