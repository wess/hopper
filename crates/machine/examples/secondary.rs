#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::{
    arm, hypervisor as hv,
    psci::{power, Reply},
  };
  use std::{
    sync::{
      atomic::{AtomicBool, Ordering},
      mpsc, Arc, Mutex,
    },
    time::Duration,
  };

  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x10000)?;
  // return through the device handler before requesting power-off
  let code = [0xd4000002u32, 0xd4000002];
  hv::write(
    &mut memory,
    0,
    &code
      .into_iter()
      .flat_map(u32::to_le_bytes)
      .collect::<Vec<_>>(),
  )?;
  let controller = Arc::new(Mutex::new(power::create(
    &[0, 1],
    std::slice::from_ref(&(0x40000000..0x40010000)),
  )?));
  let factory = hv::factory(&vm);
  std::thread::scope(|scope| -> anyhow::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let _stopper = hv::secondary::stopper(stop.clone());
    let (boot, starts) = mpsc::sync_channel(1);
    let (requests, pending) = mpsc::sync_channel(1);
    let (ready, initialized) = mpsc::sync_channel(1);
    let config = hv::secondary::Config {
      factory,
      index: 1,
      affinity: 1,
      timeout: Duration::from_secs(3),
      boot: starts,
      requests,
      wake: None,
      ready,
      stop: stop.clone(),
      power: controller.clone(),
    };
    let worker = scope.spawn(move || hv::secondary::serve(config));
    initialized.recv_timeout(Duration::from_secs(3))?;
    for context in [0x12345678abcdef42, 0x9876543210abcdef] {
      ensure!(
        matches!(
          power::call(
            &mut *controller
              .lock()
              .map_err(|_| anyhow::anyhow!("CPU power lock poisoned"))?,
            0,
            0xc4000003,
            [1, 0x40000000, context]
          ),
          Reply::CpuOn { target: 1, .. }
        ),
        "CPU_ON rejected"
      );
      boot.send(hv::secondary::Boot {
        entry: 0x40000000,
        context,
      })?;
      let request = pending.recv_timeout(Duration::from_secs(3))?;
      let hv::Exit::Exception { syndrome, .. } = request.exit else {
        anyhow::bail!("Missing guest trap")
      };
      ensure!(
        arm::decode(syndrome) == arm::Trap::Hypercall(0),
        "Wrong guest trap"
      );
      ensure!(
        hv::registers::read(&request.registers, 0)? == context,
        "Lost boot context"
      );
      let mut registers = request.registers;
      hv::registers::write(&mut registers, 0, context ^ 1)?;
      request.reply.send(hv::secondary::Response {
        registers,
        off: false,
      })?;
      let request = pending.recv_timeout(Duration::from_secs(3))?;
      ensure!(
        hv::registers::read(&request.registers, 0)? == context ^ 1,
        "Device response did not reach the guest"
      );
      request.reply.send(hv::secondary::Response {
        registers: request.registers,
        off: true,
      })?;
      let deadline = std::time::Instant::now() + Duration::from_secs(3);
      while power::call(
        &mut *controller
          .lock()
          .map_err(|_| anyhow::anyhow!("CPU power lock poisoned"))?,
        0,
        0xc4000004,
        [1, 0, 0],
      ) != Reply::Value(1)
      {
        ensure!(
          std::time::Instant::now() < deadline,
          "CPU_OFF did not park the worker"
        );
        std::thread::yield_now();
      }
    }
    stop.store(true, Ordering::Release);
    let stats = worker
      .join()
      .map_err(|_| anyhow::anyhow!("CPU owner panicked"))??;
    ensure!(
      stats.starts == 2 && stats.stops == 2 && stats.traps == 4,
      "Incomplete CPU cycles"
    );
    println!("Resident secondary CPU completed two power cycles and device response round trips");
    Ok(())
  })
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
