use super::{prepare, quiesce, trap, variables, Boot, Control, Mode, Reason, Stopped};
use crate::{
  devices::{
    flash,
    virtio::{input, pci},
  },
  hypervisor as hv, pause, platform,
};
use anyhow::Context;
use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
  },
  time::{Duration, Instant},
};

pub(super) fn execute(
  boot: Boot,
  mut devices: [Option<pci::Device>; 7],
  store: Option<&mut variables::Variables>,
  capture: Option<Arc<hv::state::group::Group>>,
  restore: Option<&hv::state::group::Checkpoint>,
  mut disks: Option<super::recovery::Disks>,
  mut poll: impl FnMut(
    &mut [Option<pci::Device>; 7],
    &mut hv::Memory<'_>,
    Mode,
  ) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  prepare::validate(&boot, &devices, restore)?;
  if let Some(disks) = &mut disks {
    let saved = restore.context("Persistent disks require a checkpoint")?;
    disks.check(saved)?;
  }
  let _binding = capture
    .as_ref()
    .map(|group| group.bind(boot.cpus as usize))
    .transpose()?;
  let vm = hv::create()?;
  let (gic, topology) = prepare::hardware(&vm, &boot)?;
  let pmu_code = if capture.is_some() || restore.is_some() {
    Some(hv::pmu::prepare(&vm, 0x3fff0000)?)
  } else {
    None
  };
  let pmu = pmu_code.as_ref().map(hv::pmu::Access::token);
  let prepare::Prepared {
    firmware,
    mut nvram,
    mut ram,
    mut variables,
    power,
    debug,
    console: guest_console,
    rtc,
  } = prepare::create(&vm, &boot, &topology, &mut devices, restore, disks.as_mut())?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  if restore.is_none() {
    hv::set(&mut cpu, 0, platform::RAM)?;
    hv::enter(&mut cpu, 0)?;
  }
  let power = Arc::new(Mutex::new(power));
  let factory = hv::factory(&vm);
  let mut console = Vec::new();
  let reason = std::thread::scope(|scope| -> anyhow::Result<Reason> {
    let stop = Arc::new(AtomicBool::new(false));
    let gate = pause::create();
    let mut phase = if restore.is_some() {
      Some(pause::request(&gate)?)
    } else {
      None
    };
    let mut paused = restore.is_some();
    let mut recovering = restore.is_some();
    let (secondary_restore, mut recovery) = match restore.and_then(|saved| saved.cpus.get(1)) {
      Some(saved) => {
        let (owner, control) = hv::secondary::recovery::create(saved);
        (Some(owner), Some(control))
      }
      None => (None, None),
    };
    let _stopper = pause::stopper(stop.clone(), gate.clone());
    let (launch, launches) = mpsc::sync_channel(1);
    let (requests, pending) = mpsc::sync_channel(1);
    let mut worker = if boot.cpus == 2 {
      let (ready, initialized) = mpsc::sync_channel(1);
      let config = hv::secondary::Config {
        pause: Some(gate.clone()),
        capture: capture.clone(),
        pmu,
        recovery: secondary_restore,
        factory,
        index: 1,
        affinity: 1,
        timeout: boot.timeout,
        boot: launches,
        requests,
        wake: Some(hv::wake(&cpu)),
        ready,
        stop: stop.clone(),
        power: power.clone(),
      };
      let worker = scope.spawn(move || hv::secondary::serve(config));
      initialized
        .recv_timeout(Duration::from_secs(5))
        .context("Initialize secondary CPU")?;
      Some(worker)
    } else {
      None
    };
    if let Some(saved) = restore {
      hv::gic::state::restore(&gic, &saved.primary.interrupts)?;
      let epoch = hv::state::clock::ticks();
      hv::state::restore_frozen(&mut cpu, &saved.cpus[0].cpu, &saved.cpus[0].clock, epoch)?;
      anyhow::ensure!(
        hv::state::read(&cpu, 0xc005)? & 0xff00ffffff == 0,
        "Restored primary CPU affinity changed"
      );
      if let Some(recovery) = &recovery {
        recovery.advance(epoch)?;
      }
      if let (Some(group), Some(generation)) = (&capture, phase) {
        // both architectural owners acknowledged restoration and remain behind recovery barriers.
        group.enter(generation)?;
      }
    }
    let mut state = trap::State {
      devices: &mut devices,
      ram: &mut ram,
      nvram: &mut nvram,
      variables: &mut variables,
      gic: &gic,
      power,
      launch,
      cpus: boot.cpus,
      debug,
      console: guest_console,
      rtc,
      tail: restore
        .map(|saved| saved.primary.actions.tail().to_vec())
        .unwrap_or_else(|| Vec::with_capacity(64)),
      enter: None,
      durable: store,
    };
    let mut next = Instant::now();
    let started = Instant::now();
    let result = hv::paced(&mut cpu, boot.timeout, Duration::from_millis(20), |cpu| {
      loop {
        anyhow::ensure!(
          started.elapsed() < boot.timeout,
          "Native guest execution timed out"
        );
        if Instant::now() >= next {
          if worker.as_ref().is_some_and(|worker| worker.is_finished()) {
            if let Some(worker) = worker.take() {
              worker
                .join()
                .map_err(|_| anyhow::anyhow!("Secondary CPU owner panicked"))?
                .context("Secondary CPU owner failed")?;
            }
            anyhow::bail!("Secondary CPU owner stopped unexpectedly");
          }
          if let Some(disks) = &mut disks {
            (disks.authorize)()?;
          }
          let was_paused = paused;
          let collecting = capture
            .as_ref()
            .map(|group| group.collecting())
            .transpose()?
            .unwrap_or(false);
          let control = if collecting {
            Control::Pause
          } else {
            poll(
              state.devices,
              state.ram,
              if paused { Mode::Paused } else { Mode::Running },
            )?
          };
          if let Some(disks) = &mut disks {
            (disks.authorize)()?;
          }
          match control {
            Control::Stop => return Ok(Reason::Stopped),
            Control::Pause if !paused => {
              let generation = pause::request(&gate)?;
              if boot.cpus == 2 {
                if let Some(reason) =
                  quiesce::wait(&mut state, &pending, &gate, generation, || {
                    worker.as_ref().is_some_and(|worker| worker.is_finished())
                  })?
                {
                  return Ok(reason);
                }
              }
              if let Some(group) = &capture {
                group.enter(generation)?;
              }
              phase = Some(generation);
              paused = true;
            }
            Control::Continue if paused => {
              if recovering {
                let saved = restore.context("Missing restored CPU state")?;
                let epoch = hv::state::clock::ticks();
                hv::state::group::resume(
                  cpu,
                  &saved.cpus[0],
                  pmu.context("Missing PMU recovery mapping")?,
                  epoch,
                )?;
                if let Some(recovery) = recovery.take() {
                  recovery.advance(epoch)?;
                }
                state.enter = saved.primary.actions.resume(Instant::now())?;
                recovering = false;
              }
              if let Some(group) = &capture {
                group.leave();
              }
              phase = None;
              pause::resume(&gate);
              paused = false;
            }
            _ => {}
          }
          if !paused && state.enter.is_some_and(|time| Instant::now() >= time) {
            if let Some(keyboard) = &mut state.devices[2] {
              pci::send_input(
                keyboard,
                state.ram,
                &[
                  input::Event {
                    kind: 1,
                    code: 28,
                    value: 1,
                  },
                  input::SYN,
                  input::Event {
                    kind: 1,
                    code: 28,
                    value: 0,
                  },
                  input::SYN,
                ],
              )?;
            }
            state.enter = None;
          }
          if !collecting && !recovering {
            trap::interrupts(&mut state)?;
          }
          if let (Some(group), Some(generation)) = (&capture, phase) {
            prepare::capture(
              group,
              generation,
              cpu,
              pmu.context("Missing PMU capture mapping")?,
              &firmware,
              &state,
              if recovering { restore } else { None },
            )?;
          }

          if !was_paused && paused {
            // every CPU is held now; deliver the paused callback before waiting another tick.
            next = Instant::now();
            continue;
          }
          next = Instant::now() + Duration::from_millis(20);
        }
        if paused {
          std::thread::sleep(Duration::from_millis(5));
          continue;
        }
        if let Ok(request) = pending.try_recv() {
          if let Some(reason) = quiesce::dispatch(&mut state, request)? {
            return Ok(reason);
          }
          continue;
        }
        let exit = hv::run(cpu)?;
        if matches!(exit, hv::Exit::Canceled) {
          continue;
        }
        let before = hv::registers::capture(cpu)?;
        let mut registers = before.clone();
        let action = trap::dispatch(&mut state, 0, exit, &mut registers)?;
        if let Some(reason) = action.reason {
          return Ok(reason);
        }
        hv::registers::apply(cpu, &before, &registers)?;
      }
    });
    drop(recovery.take());
    stop.store(true, Ordering::Release);
    pause::resume(&gate);
    if let Some(worker) = worker {
      let joined = worker
        .join()
        .map_err(|_| anyhow::anyhow!("Secondary CPU owner panicked"))?;
      if result.is_ok() {
        joined?;
      }
    }
    console = state.tail;
    result
  })?;
  // no guest CPU is executing while a physical framebuffer is sampled.
  for device in devices.iter_mut().flatten() {
    pci::refresh(device, &ram)?;
  }
  if let Some(disks) = &mut disks {
    (disks.authorize)()?;
  }
  Ok(Stopped {
    reason,
    console,
    variables: flash::bytes(&variables).to_vec(),
    devices,
  })
}
