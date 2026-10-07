use super::{Action, Pending, Vm};
use anyhow::{ensure, Context};
use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::{mpsc, oneshot, Notify};

pub type Check = Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>;

#[derive(Clone)]
pub struct Client(mpsc::Sender<Request>, Arc<Notify>);

/// the owner must be driven on the VM queue, independent of viewer lifetime.
///
/// ```compile_fail
/// fn move_owner(owner: machine::vz::queue::Owner) {
///   std::thread::spawn(move || drop(owner));
/// }
/// ```
pub struct Owner {
  receive: mpsc::Receiver<Request>,
  wake: Arc<Notify>,
  machines: BTreeMap<String, Vm>,
  pending: BTreeMap<String, Operation>,
}

enum Request {
  Transition(Transition),
  Status(Query),
}

#[derive(Clone, Copy, Debug)]
pub struct Status {
  pub state: super::VZVirtualMachineState,
  pub busy: bool,
}

struct Query {
  id: String,
  check: Check,
  send: oneshot::Sender<anyhow::Result<Option<Status>>>,
}

struct Transition {
  id: String,
  action: Action,
  check: Option<Check>,
  send: oneshot::Sender<anyhow::Result<()>>,
}

struct Operation {
  pending: Pending,
  send: oneshot::Sender<anyhow::Result<()>>,
}

pub fn channel() -> (Client, Owner) {
  let (send, receive) = mpsc::channel(16);
  let wake = Arc::new(Notify::new());
  (
    Client(send, wake.clone()),
    Owner {
      receive,
      wake,
      machines: BTreeMap::new(),
      pending: BTreeMap::new(),
    },
  )
}

impl Client {
  pub async fn transition(&self, id: &str, action: Action) -> anyhow::Result<()> {
    self.scoped(id, action, None).await
  }

  pub async fn transition_checked(
    &self,
    id: &str,
    action: Action,
    check: Check,
  ) -> anyhow::Result<()> {
    self.scoped(id, action, Some(check)).await
  }

  pub async fn status_checked(&self, id: &str, check: Check) -> anyhow::Result<Option<Status>> {
    validate(id)?;
    check()?;
    let (send, receive) = oneshot::channel();
    self
      .0
      .send(Request::Status(Query {
        id: id.into(),
        check: check.clone(),
        send,
      }))
      .await
      .context("VZ owner is unavailable")?;
    self.1.notify_one();
    let result = receive.await.context("VZ status ended without a result")?;
    check()?;
    result
  }

  async fn scoped(&self, id: &str, action: Action, check: Option<Check>) -> anyhow::Result<()> {
    validate(id)?;
    if let Some(check) = &check {
      check()?;
    }
    let (send, receive) = oneshot::channel();
    self
      .0
      .send(Request::Transition(Transition {
        id: id.into(),
        action,
        check: check.clone(),
        send,
      }))
      .await
      .context("VZ owner is unavailable")?;
    self.1.notify_one();
    let result = receive
      .await
      .context("VZ operation ended without a result")?;
    if let Some(check) = &check {
      check()?;
    }
    result
  }
}

impl Owner {
  pub fn wake(&self) -> Arc<Notify> {
    self.wake.clone()
  }

  pub fn active(&self) -> bool {
    !self.pending.is_empty()
  }

  pub fn insert(&mut self, id: &str, vm: Vm) -> anyhow::Result<()> {
    validate(id)?;
    ensure!(
      !self.machines.contains_key(id),
      "VZ identity is already owned"
    );
    self.machines.insert(id.into(), vm);
    Ok(())
  }

  pub fn state(&self, id: &str) -> anyhow::Result<objc2_virtualization::VZVirtualMachineState> {
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    Ok(super::state(vm))
  }

  pub fn retire(&mut self, id: &str) -> anyhow::Result<()> {
    ensure!(
      !self.pending.contains_key(id),
      "VZ operation is still active"
    );
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    ensure!(
      !vm.installing.get(),
      "macOS installation still owns this VM"
    );
    ensure!(
      super::state(vm) == objc2_virtualization::VZVirtualMachineState::Stopped,
      "Stop the VZ machine before retiring ownership"
    );
    self.machines.remove(id);
    Ok(())
  }

  pub fn tick(&mut self) {
    let mut finished = Vec::new();
    for (id, operation) in &self.pending {
      match super::poll(&operation.pending) {
        Ok(None) => {}
        Ok(Some(result)) => finished.push((id.clone(), result)),
        Err(error) => finished.push((id.clone(), Err(error))),
      }
    }
    for (id, result) in finished {
      if let Some(operation) = self.pending.remove(&id) {
        let _ = operation.send.send(result);
      }
    }
    for _ in 0..16 {
      let Ok(request) = self.receive.try_recv() else {
        break;
      };
      let request = match request {
        Request::Transition(request) => request,
        Request::Status(query) => {
          if !query.send.is_closed() {
            let result = (query.check)().map(|()| {
              self.machines.get(&query.id).map(|vm| Status {
                state: super::state(vm),
                busy: self.pending.contains_key(&query.id) || vm.installing.get(),
              })
            });
            let _ = query.send.send(result);
          }
          continue;
        }
      };
      if request.send.is_closed() {
        continue;
      }
      let result = (|| {
        if let Some(check) = &request.check {
          check()?;
        }
        ensure!(
          !self.pending.contains_key(&request.id),
          "Another VZ operation is active"
        );
        let vm = self
          .machines
          .get(&request.id)
          .context("VZ machine is not owned")?;
        super::scoped_transition(vm, request.action, request.check.clone())
      })();
      match result {
        Ok(pending) => {
          self.pending.insert(
            request.id,
            Operation {
              pending,
              send: request.send,
            },
          );
        }
        Err(error) => {
          let _ = request.send.send(Err(error));
        }
      }
    }
  }
}

fn validate(id: &str) -> anyhow::Result<()> {
  ensure!(
    !id.is_empty()
      && id.len() <= 128
      && id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
    "Invalid VZ identity"
  );
  Ok(())
}
