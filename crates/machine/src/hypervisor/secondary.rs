use super::{
  affinity, check, create_cpu, enter, ffi, get, paced, registers, set, CpuFactory, Exit,
};
use crate::psci::power;
use anyhow::{ensure, Context};
use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError},
    Arc, Mutex,
  },
  time::{Duration, Instant},
};

pub struct Boot {
  pub entry: u64,
  pub context: u64,
}

pub struct Request {
  pub exit: Exit,
  pub registers: registers::Registers,
  pub reply: SyncSender<Response>,
}

pub struct Response {
  pub registers: registers::Registers,
  pub off: bool,
}

pub struct Config<'vm> {
  pub factory: CpuFactory<'vm>,
  pub index: usize,
  pub affinity: u64,
  pub timeout: Duration,
  pub boot: Receiver<Boot>,
  pub requests: SyncSender<Request>,
  pub wake: Option<super::Wake>,
  pub ready: SyncSender<()>,
  pub stop: Arc<AtomicBool>,
  pub power: Arc<Mutex<power::Power>>,
  pub pause: Option<Arc<crate::pause::Pause>>,
}

#[derive(Debug, Default)]
pub struct Stats {
  pub starts: u64,
  pub traps: u64,
  pub stops: u64,
  pub instruction: u64,
}

pub struct Stopper(Arc<AtomicBool>);

pub fn stopper(flag: Arc<AtomicBool>) -> Stopper {
  Stopper(flag)
}

impl Drop for Stopper {
  fn drop(&mut self) {
    self.0.store(true, Ordering::Release);
  }
}

pub fn serve(config: Config<'_>) -> anyhow::Result<Stats> {
  let mut stats = Stats::default();
  let mut cpu = create_cpu(config.factory)?;
  affinity(&mut cpu, config.affinity)?;
  // cpu_off parks the existing vcpu; the native gic topology cannot change after boot.
  let controls = [0xc080, 0xdf19, 0xdf11].map(|register| {
    let mut value = 0;
    check(
      unsafe { ffi::hv_vcpu_get_sys_reg(cpu.id, register, &mut value) },
      "Read CPU boot control",
    )?;
    Ok::<_, anyhow::Error>((register, value))
  });
  let controls = controls.into_iter().collect::<anyhow::Result<Vec<_>>>()?;
  config.ready.send(())?;
  let interval = Duration::from_millis(20);
  loop {
    if !checkpoint(&config)? {
      return Ok(stats);
    }
    let boot = match config.boot.recv_timeout(interval) {
      Ok(boot) => boot,
      Err(RecvTimeoutError::Timeout) => continue,
      Err(RecvTimeoutError::Disconnected) => return Ok(stats),
    };
    for &(register, value) in &controls {
      check(
        unsafe { ffi::hv_vcpu_set_sys_reg(cpu.id, register, value) },
        "Restore CPU boot control",
      )?;
    }
    enter(&mut cpu, boot.entry)?;
    set(&mut cpu, 0, boot.context)?;
    power::started(
      &mut *config
        .power
        .lock()
        .map_err(|_| anyhow::anyhow!("CPU power lock poisoned"))?,
      config.index,
    )?;
    stats.starts += 1;
    let off = paced(
      &mut cpu,
      config.timeout,
      interval,
      |cpu| -> anyhow::Result<bool> {
        let started = Instant::now();
        loop {
          if !checkpoint(&config)? {
            return Ok(false);
          }
          let exit = super::run(cpu)?;
          stats.instruction = get(cpu, ffi::PC)?;
          if matches!(exit, Exit::Canceled) {
            continue;
          }
          let before = registers::capture(cpu)?;
          let (reply, response) = std::sync::mpsc::sync_channel(1);
          let mut request = Request {
            exit,
            registers: before.clone(),
            reply,
          };
          loop {
            if !checkpoint(&config)? {
              return Ok(false);
            }
            ensure!(
              started.elapsed() < config.timeout,
              "Secondary CPU request timed out"
            );
            match config.requests.try_send(request) {
              Ok(()) => {
                if let Some(wake) = &config.wake {
                  ensure!(
                    super::request_exit(wake)?,
                    "Device handler CPU was destroyed"
                  );
                }
                break;
              }
              Err(TrySendError::Full(pending)) => {
                request = pending;
                std::thread::sleep(interval);
              }
              Err(TrySendError::Disconnected(_)) => return Ok(false),
            }
          }
          stats.traps += 1;
          loop {
            if !checkpoint(&config)? {
              return Ok(false);
            }
            ensure!(
              started.elapsed() < config.timeout,
              "Secondary CPU response timed out"
            );
            match response.recv_timeout(interval) {
              Ok(response) => {
                if response.off {
                  return Ok(true);
                }
                registers::apply(cpu, &before, &response.registers)?;
                break;
              }
              Err(RecvTimeoutError::Timeout) => continue,
              Err(RecvTimeoutError::Disconnected) => return Ok(false),
            }
          }
        }
      },
    )
    .context("Run secondary CPU")?;
    if !off {
      return Ok(stats);
    }

    power::stopped(
      &mut *config
        .power
        .lock()
        .map_err(|_| anyhow::anyhow!("CPU power lock poisoned"))?,
      config.index,
    )?;
    stats.stops += 1;
    ensure!(stats.stops <= stats.starts, "Invalid CPU power cycle count");
  }
}

fn checkpoint(config: &Config<'_>) -> anyhow::Result<bool> {
  match &config.pause {
    Some(pause) => crate::pause::checkpoint(pause, &config.stop),
    None => Ok(!config.stop.load(Ordering::Acquire)),
  }
}
