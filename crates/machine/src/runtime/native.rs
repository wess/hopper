use super::{trap, validate, variables, Boot, Control, Mode, Reason};
use crate::{
  acpi, debug,
  devices::{
    flash,
    virtio::{input, pci},
  },
  hypervisor as hv, pause, platform, psci,
};
use anyhow::Context;
use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
  },
  time::{Duration, Instant},
};

pub struct Stopped {
  pub reason: Reason,
  pub variables: Vec<u8>,
  pub devices: [Option<pci::Device>; 7],
}

/// run on the VM owner thread; callbacks handle guest I/O and request a clean stop.
pub fn run(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  execute(boot, devices, None, poll)
}

pub fn run_persistent(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  store: &mut variables::Variables,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  anyhow::ensure!(
    boot.variables == variables::bytes(store),
    "Boot variables do not match their persistent store"
  );
  execute(boot, devices, Some(store), poll)
}

fn execute(
  boot: Boot,
  mut devices: [Option<pci::Device>; 7],
  store: Option<&mut variables::Variables>,
  mut poll: impl FnMut(
    &mut [Option<pci::Device>; 7],
    &mut hv::Memory<'_>,
    Mode,
  ) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  validate(&boot)?;
  let vm = hv::create()?;
  let gic = hv::gic::create_msi(
    &vm,
    platform::DISTRIBUTOR,
    platform::REDISTRIBUTOR,
    platform::MSI,
    64,
    32,
  )?;
  let topology = platform::Topology {
    memory: boot.memory,
    cpus: boot.cpus,
    distributor_size: gic.distributor_size as u64,
    redistributor_size: gic.redistributor_size as u64,
    msi: gic.msi.as_ref().map(|msi| platform::Msi {
      size: msi.size as u64,
      first: msi.first,
      count: msi.count,
    }),
  };
  let tree = platform::tree(&topology)?;
  let tables = acpi::bundle(&topology)?;
  let mut firmware = hv::memory(&vm, 0, 0x4000000)?;
  hv::write(&mut firmware, 0, &boot.firmware)?;
  hv::protect(&firmware, 5)?;
  let mut data = vec![0xff; 0x4000000];
  data[..boot.variables.len()].copy_from_slice(&boot.variables);
  let mut variables = flash::create(data, 0x40000)?;
  let mut nvram = hv::memory(&vm, 0x4000000, 0x4000000)?;
  hv::write(&mut nvram, 0, flash::bytes(&variables))?;
  hv::protect(&nvram, 1)?;
  let mut ram = hv::memory(&vm, platform::RAM, topology.memory as usize)?;
  hv::write(&mut ram, 0, &tree)?;
  hv::write(&mut ram, (acpi::BASE - platform::RAM) as usize, &tables)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::set(&mut cpu, 0, platform::RAM)?;
  hv::enter(&mut cpu, 0)?;
  let power = Arc::new(Mutex::new(psci::power::create(
    &(0..boot.cpus as u64).collect::<Vec<_>>(),
    &[0..0x4000000, platform::RAM..platform::RAM + topology.memory],
  )?));
  let factory = hv::factory(&vm);
  let reason = std::thread::scope(|scope| -> anyhow::Result<Reason> {
    let stop = Arc::new(AtomicBool::new(false));
    let gate = pause::create();
    let _stopper = pause::stopper(stop.clone(), gate.clone());
    let (launch, launches) = mpsc::sync_channel(1);
    let (requests, pending) = mpsc::sync_channel(1);
    let worker = if boot.cpus == 2 {
      let (ready, initialized) = mpsc::sync_channel(1);
      let config = hv::secondary::Config {
        pause: Some(gate.clone()),
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
    let mut state = trap::State {
      devices: &mut devices,
      ram: &mut ram,
      nvram: &mut nvram,
      variables: &mut variables,
      gic: &gic,
      power,
      launch,
      cpus: boot.cpus,
      debug: (0..boot.cpus).map(|_| debug::State::default()).collect(),
      console: Default::default(),
      tail: Vec::with_capacity(64),
      enter: None,
      durable: store,
    };
    let mut paused = false;
    let mut next = Instant::now();
    let started = Instant::now();
    let result = hv::paced(&mut cpu, boot.timeout, Duration::from_millis(20), |cpu| {
      loop {
        anyhow::ensure!(
          started.elapsed() < boot.timeout,
          "Native guest execution timed out"
        );
        if Instant::now() >= next {
          anyhow::ensure!(
            worker.as_ref().is_none_or(|worker| !worker.is_finished()),
            "Secondary CPU owner stopped unexpectedly"
          );
          match poll(
            state.devices,
            state.ram,
            if paused { Mode::Paused } else { Mode::Running },
          )? {
            Control::Stop => return Ok(Reason::Stopped),
            Control::Pause if !paused => {
              if boot.cpus == 2 {
                let generation = pause::request(&gate)?;
                pause::wait(&gate, generation, Duration::from_secs(5))?;
              }
              paused = true;
            }
            Control::Continue if paused => {
              pause::resume(&gate);
              paused = false;
            }
            _ => {}
          }
          if state.enter.is_some_and(|time| Instant::now() >= time) {
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
          trap::interrupts(&mut state)?;
          next = Instant::now() + Duration::from_millis(20);
        }
        if paused {
          std::thread::sleep(Duration::from_millis(5));
          continue;
        }
        let request = pending.try_recv().ok();
        let caller = usize::from(request.is_some());
        let exit = if let Some(request) = &request {
          request.exit
        } else {
          hv::run(cpu)?
        };
        if matches!(exit, hv::Exit::Canceled) {
          continue;
        }
        let before = if let Some(request) = &request {
          request.registers.clone()
        } else {
          hv::registers::capture(cpu)?
        };
        let mut registers = before.clone();
        let action = trap::dispatch(&mut state, caller, exit, &mut registers)?;
        if let Some(reason) = action.reason {
          return Ok(reason);
        }
        if let Some(request) = request {
          request.reply.send(hv::secondary::Response {
            registers,
            off: action.off,
          })?;
        } else {
          hv::registers::apply(cpu, &before, &registers)?;
        }
      }
    });
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
    result
  })?;
  // no guest CPU is executing while a physical framebuffer is sampled.
  for device in devices.iter_mut().flatten() {
    pci::refresh(device, &ram)?;
  }
  Ok(Stopped {
    reason,
    variables: flash::bytes(&variables).to_vec(),
    devices,
  })
}
