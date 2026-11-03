use super::checkpoint;
use anyhow::{ensure, Context};
use machine::{
  devices::virtio::pci,
  hypervisor::{self as hv, state::group},
  runtime::{self, Boot, Control, Mode},
};
use std::time::{Duration, Instant};

pub fn run() -> anyhow::Result<()> {
  let mut code = vec![0xd2a12006u32, 0x52806028, 0xb90030c8];
  for &byte in b"cdboot.efi" {
    code.extend([0x52800007 | (u32::from(byte) << 5), 0xb90000c7]);
  }
  code.extend([0xd2a80024, 0xf9400085, 0x910004a5, 0xf9000085, 0x17fffffd]);
  let firmware = code
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
  let boot = || Boot {
    firmware: firmware.clone(),
    variables: vec![0xff],
    memory: 0x10000000,
    cpus: 1,
    timeout: Duration::from_secs(6),
  };
  let devices = || -> anyhow::Result<[Option<pci::Device>; 7]> {
    let mut devices = std::array::from_fn(|_| None);
    devices[2] = Some(checkpoint::device()?);
    Ok(devices)
  };
  let group = group::create(1)?;
  let mut pending = None;
  let mut saved = None;
  let stopped = runtime::run_captured(boot(), devices()?, group.clone(), |devices, ram, mode| {
    if mode == Mode::Paused {
      if pending.is_none() {
        checkpoint::prepare_transport(devices[2].as_mut().context("Missing keyboard")?, ram)?;
        pending = Some(group.request()?);
      }
      if let Some(captured) = pending.as_mut().context("Missing action capture")?.poll()? {
        saved = Some(captured);
        return Ok(Control::Stop);
      }
      return Ok(Control::Pause);
    }
    let mut count = [0; 8];
    hv::read(ram, 0x10000, &mut count)?;
    Ok(if count != [0; 8] {
      Control::Pause
    } else {
      Control::Continue
    })
  })?;
  ensure!(stopped.console == b"cdboot.efi");
  drop(stopped);
  let saved = super::records::reopen(saved.context("Missing pending-action checkpoint")?, devices)?;
  ensure!(saved.primary.actions.tail() == b"cdboot.efi");
  let now = Instant::now();
  let delay = saved
    .primary
    .actions
    .resume(now)?
    .context("Boot-key delay was not captured")?
    .duration_since(now);
  ensure!(delay > Duration::from_millis(200));
  let mut initialized = false;
  let mut resumed = false;
  let mut delivered = None;
  let mut since = Instant::now();
  let stopped = runtime::run_restored(boot(), devices()?, &saved, |devices, ram, mode| {
    let keyboard = devices[2].as_mut().context("Missing restored keyboard")?;
    if !resumed {
      ensure!(mode == Mode::Paused);
      if !initialized {
        hv::write(ram, 0x30102, &8u16.to_le_bytes())?;
        pci::write(keyboard, ram, pci::NOTIFY, 2, 0)?;
        initialized = true;
        since = Instant::now();
      }
      ensure!(
        pci::completed(keyboard) == 4,
        "Boot key arrived during held recovery"
      );
      if since.elapsed() < Duration::from_millis(1200) {
        return Ok(Control::Pause);
      }
      resumed = true;
      since = Instant::now();
      return Ok(Control::Continue);
    }
    if since.elapsed() < delay.saturating_sub(Duration::from_millis(100)) {
      ensure!(
        pci::completed(keyboard) == 4,
        "Recovery hold consumed the pending delay"
      );
    }
    if pci::completed(keyboard) == 8 {
      let mut events = [0; 32];
      hv::read(ram, 0x31020, &mut events)?;
      ensure!(&events[..8] == [1, 0, 28, 0, 1, 0, 0, 0]);
      ensure!(&events[8..16] == [0; 8]);
      ensure!(&events[16..24] == [1, 0, 28, 0, 0, 0, 0, 0]);
      ensure!(&events[24..] == [0; 8]);
      let at = delivered.get_or_insert_with(Instant::now);
      if at.elapsed() >= Duration::from_millis(100) {
        hv::write(ram, 0x30102, &12u16.to_le_bytes())?;
        pci::write(keyboard, ram, pci::NOTIFY, 2, 0)?;
        ensure!(
          pci::completed(keyboard) == 8,
          "Boot key queued a duplicate chord"
        );
        return Ok(Control::Stop);
      }
    }
    Ok(Control::Continue)
  })?;
  ensure!(resumed && delivered.is_some() && stopped.console == b"cdboot.efi");
  drop(stopped);
  println!("Pending boot key survives VM teardown and 1.2-second held recovery, retains its remaining delay and delivers one complete press/release chord after resume");
  Ok(())
}
