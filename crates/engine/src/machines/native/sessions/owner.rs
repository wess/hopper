use super::{
  super::{Client, State},
  Slot,
};
use tokio::sync::watch;

pub(super) struct Session {
  pub client: Client,
  released: watch::Receiver<bool>,
}

pub(super) fn own(client: Client, lease: store::lock::Lease) -> Session {
  let mut ended = client.ended.clone();
  let (release, released) = watch::channel(false);
  tokio::spawn(async move {
    while !*ended.borrow_and_update() {
      if ended.changed().await.is_err() {
        break;
      }
    }
    drop(lease);
    release.send_replace(true);
  });
  Session { client, released }
}

pub(super) async fn started(
  manager: &crate::machines::Machines,
  id: &str,
  client: Client,
  lease: store::lock::Lease,
  completion: Option<model::native::Installation>,
) -> anyhow::Result<Session> {
  let session = own(client, lease);
  if let Some(completion) = completion {
    if let Err(error) = super::super::installation::save(manager, id, completion) {
      let _ = super::super::request(&session.client, model::native::Command::Stop {}).await;
      let _ = finished(&session).await;
      return Err(error);
    }
  }
  Ok(session)
}

pub(super) async fn finished(session: &Session) -> anyhow::Result<()> {
  super::super::finished(&session.client).await?;
  let mut released = session.released.clone();
  while !*released.borrow_and_update() {
    released
      .changed()
      .await
      .map_err(|_| anyhow::anyhow!("Native ownership lease cleanup failed"))?;
  }
  Ok(())
}

pub(super) fn publish(slot: &Slot, client: &Client) -> u64 {
  let mut epoch = slot
    .generation
    .lock()
    .unwrap_or_else(|error| error.into_inner());
  *epoch = epoch.wrapping_add(1);
  let current = *epoch;
  let generation = slot.generation.clone();
  let state = slot.state.clone();
  let mut source = client.state.clone();
  state.send_replace(source.borrow().clone());
  drop(epoch);
  // receivers observe hardware without retaining a command sender or the VM registry.
  tokio::spawn(async move {
    while source.changed().await.is_ok() {
      let epoch = generation.lock().unwrap_or_else(|error| error.into_inner());
      if *epoch != current {
        break;
      }
      state.send_replace(source.borrow_and_update().clone());
    }
  });
  current
}

pub(super) fn retire(slot: &Slot, state: State) {
  let mut epoch = slot
    .generation
    .lock()
    .unwrap_or_else(|error| error.into_inner());
  *epoch = epoch.wrapping_add(1);
  slot.state.send_replace(state);
}
