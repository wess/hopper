use super::{Input, Sessions};
use crate::machines::native::State;
use std::{sync::atomic::Ordering, time::Duration};
use tokio::{
  sync::{watch, OwnedMutexGuard},
  time::{Instant, MissedTickBehavior},
};

pub(super) fn start(
  lease: &Input,
  mut gate: OwnedMutexGuard<Option<u64>>,
  mut release: watch::Receiver<bool>,
  mut activity: watch::Receiver<Instant>,
  state: watch::Receiver<State>,
  end: watch::Sender<Option<Result<(), String>>>,
) {
  let registry = lease.registry.clone();
  let id = lease.id.clone();
  let actor = lease.actor;
  let policy = lease.policy;
  let generation = lease.generation.clone();
  let epoch = lease.epoch;
  let active = lease.active.clone();
  tokio::spawn(async move {
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
      let deadline = *activity.borrow_and_update() + Duration::from_secs(5);
      tokio::select! {
        biased;
        changed = release.changed() => {
          if changed.is_err() || *release.borrow_and_update() { break; }
        }
        changed = activity.changed() => { if changed.is_err() { break; } }
        _ = tokio::time::sleep_until(deadline) => { break; }
        _ = tick.tick() => {
          let allowed = registry.upgrade().is_some_and(|registry| registry.manager.machine(&id, actor).is_ok_and(|machine| policy.is_none_or(|epoch| machine.agent_generation == epoch)));
          let current = *generation.lock().unwrap_or_else(|error| error.into_inner()) == epoch;
          if !allowed || !current || !matches!(*state.borrow(), State::Running) { break; }
        }
      }
    }
    active.store(false, Ordering::Release);
    let result = if let Some(registry) = registry.upgrade() {
      Sessions { inner: registry }.release_input(&id, epoch).await
    } else {
      Ok(())
    };
    *gate = result.is_err().then_some(epoch);
    drop(gate);
    end.send_replace(Some(result.map_err(|error| format!("{error:#}"))));
  });
}
