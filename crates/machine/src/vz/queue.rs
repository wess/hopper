mod install;
mod jobs;
mod status;
pub use install::Installation;
pub use status::Status;

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
  installations: BTreeMap<String, install::Operation>,
  generation: u64,
  jobs: BTreeMap<String, jobs::Link>,
  jobs_due: std::time::Instant,
}

enum Request {
  Transition(Transition),
  Status(Query),
  Install(install::RequestInstall),
  Guest(jobs::Query),
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
      installations: BTreeMap::new(),
      generation: 0,
      jobs: BTreeMap::new(),
      jobs_due: std::time::Instant::now(),
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
      || !self.installations.is_empty()
      || !self.jobs.is_empty()
      || self.machines.values().any(super::bootstrap::active)
  }

  pub fn insert(&mut self, id: &str, vm: Vm) -> anyhow::Result<()> {
    validate(id)?;
    ensure!(
      !self.machines.contains_key(id),
      "VZ identity is already owned"
    );
    let generation = self.next_generation()?;
    ensure!(
      vm.guest_boot
        .as_ref()
        .is_none_or(|boot| boot.machine == id && boot.generation == generation),
      "Private boot identity or generation does not match admission"
    );
    self.generation = generation;
    vm.generation.set(self.generation);
    self.machines.insert(id.into(), vm);
    Ok(())
  }

  pub fn next_generation(&self) -> anyhow::Result<u64> {
    self
      .generation
      .checked_add(1)
      .context("VZ generation exhausted")
  }

  pub fn requires_replacement(&self, id: &str) -> anyhow::Result<bool> {
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    Ok(vm.guest_boot.is_some() && vm.started.get())
  }

  pub fn commands(
    &self,
    id: &str,
    policy: u64,
    check: Check,
  ) -> anyhow::Result<crate::guest::Connection<super::socket::Listener>> {
    check()?;
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    let material = vm
      .guest_material
      .as_ref()
      .context("This VM has no owned guest credentials")?;
    crate::guest::connect_borrowed(
      self.guest_socket(id, check)?,
      material,
      policy,
      std::time::Instant::now(),
    )
  }

  pub fn state(&self, id: &str) -> anyhow::Result<objc2_virtualization::VZVirtualMachineState> {
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    Ok(super::state(vm))
  }

  pub fn display(&self, id: &str) -> anyhow::Result<super::Display> {
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    super::display::create(vm)
  }

  pub fn guest_socket(&self, id: &str, check: Check) -> anyhow::Result<super::socket::Listener> {
    ensure!(
      !self.pending.contains_key(id) && !self.installations.contains_key(id),
      "VZ operation is still active"
    );
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    super::socket::listen(vm, check)
  }

  /// command clients and their socket stay on the VM queue.
  /// the check must retain the original runtime and policy generations.
  ///
  /// ```compile_fail
  /// fn move_commands(client: machine::guest::Connection<machine::vz::socket::Listener>) {
  ///   std::thread::spawn(move || drop(client));
  /// }
  /// ```
  pub fn guest_commands(
    &self,
    id: &str,
    material: model::guest::wire::auth::Material,
    policy: u64,
    check: Check,
  ) -> anyhow::Result<crate::guest::Connection<super::socket::Listener>> {
    ensure!(
      material.boot.machine == id && material.boot.guest == model::GuestOs::Linux,
      "Guest command material does not match the owned Linux VM"
    );
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    ensure!(
      vm.guest_boot
        .as_ref()
        .is_none_or(|boot| boot == &material.boot),
      "Guest command generation does not match the private boot channel"
    );
    crate::guest::connect(
      self.guest_socket(id, check)?,
      material,
      policy,
      std::time::Instant::now(),
    )
  }

  pub fn installer(&self, id: &str) -> anyhow::Result<bool> {
    Ok(
      self
        .machines
        .get(id)
        .context("VZ machine is not owned")?
        .installer,
    )
  }

  pub fn can_replace(&self, id: &str) -> anyhow::Result<()> {
    ensure!(
      !self.pending.contains_key(id) && !self.installations.contains_key(id),
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
    Ok(())
  }

  pub fn can_retire(&self, id: &str) -> anyhow::Result<()> {
    self.can_replace(id)?;
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    ensure!(
      !vm.displaying.get(),
      "Close the VM display before retiring ownership"
    );
    ensure!(
      !vm.listening.get(),
      "Close the guest transport before retiring ownership"
    );
    Ok(())
  }

  pub fn retire(&mut self, id: &str) -> anyhow::Result<()> {
    self.can_retire(id)?;
    self.machines.remove(id);
    Ok(())
  }

  pub fn tick(&mut self) {
    for vm in self.machines.values() {
      super::bootstrap::tick(vm);
    }
    self.tick_installations();
    self.tick_jobs();
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
        Request::Install(request) => {
          self.install(request);
          continue;
        }
        Request::Status(query) => {
          if !query.send.is_closed() {
            let result = (query.check)().map(|()| {
              self
                .machines
                .get(&query.id)
                .and_then(|_| self.inspect(&query.id).ok())
            });
            let _ = query.send.send(result);
          }
          continue;
        }
        Request::Guest(query) => {
          self.guest_request(query);
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
        let vm = self
          .machines
          .get(&request.id)
          .context("VZ machine is not owned")?;
        if matches!(request.action, Action::Stop) {
          vm.stop_requested.set(true);
        }
        ensure!(
          !self.pending.contains_key(&request.id) && !self.installations.contains_key(&request.id),
          "Another VZ operation is active"
        );
        if matches!(request.action, Action::Stop | Action::Pause) {
          self.jobs.remove(&request.id);
        }
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
