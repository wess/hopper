use super::{trap, Reason};
use crate::{hypervisor as hv, pause};
use anyhow::ensure;
use std::{
  sync::mpsc::{Receiver, TryRecvError},
  time::Duration,
};

pub(super) fn dispatch(
  state: &mut trap::State<'_, '_>,
  request: hv::secondary::Request,
) -> anyhow::Result<Option<Reason>> {
  let mut registers = request.registers;
  let action = trap::dispatch(state, 1, request.exit, &mut registers)?;
  if action.reason.is_some() {
    return Ok(action.reason);
  }
  request.reply.send(hv::secondary::Response {
    registers,
    off: action.off,
  })?;
  Ok(None)
}

pub(super) fn wait(
  state: &mut trap::State<'_, '_>,
  pending: &Receiver<hv::secondary::Request>,
  gate: &pause::Pause,
  generation: u64,
  finished: impl Fn() -> bool,
) -> anyhow::Result<Option<Reason>> {
  let reason = pause::wait_with(gate, generation, Duration::from_secs(5), || {
    ensure!(!finished(), "Secondary CPU owner stopped during pause");
    match pending.try_recv() {
      Ok(request) => dispatch(state, request),
      Err(TryRecvError::Empty) => Ok(None),
      Err(TryRecvError::Disconnected) => {
        anyhow::bail!("Secondary CPU request channel closed during pause")
      }
    }
  })?;
  if reason.is_none() {
    ensure!(
      matches!(pending.try_recv(), Err(TryRecvError::Empty)),
      "Secondary CPU acknowledged pause with a pending trap"
    );
  }
  Ok(reason)
}
