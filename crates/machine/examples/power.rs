#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::{
    arm, hypervisor as hv,
    psci::{power, Reply},
  };
  use std::{
    sync::{mpsc, Arc, Mutex, MutexGuard},
    time::Duration,
  };

  fn lock(power: &Mutex<power::Power>) -> anyhow::Result<MutexGuard<'_, power::Power>> {
    power
      .lock()
      .map_err(|_| anyhow::anyhow!("CPU power state lock poisoned"))
  }
  fn hypercall(cpu: &mut hv::Cpu<'_>) -> anyhow::Result<()> {
    let hv::Exit::Exception { syndrome, .. } = hv::run(cpu)? else {
      anyhow::bail!("Guest did not reach a hypercall");
    };
    ensure!(
      arm::decode(syndrome) == arm::Trap::Hypercall(0),
      "Unexpected guest trap"
    );
    Ok(())
  }
  fn bytes(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
  }

  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x10000)?;
  // primary calls cpu_on, waits for the secondary's context publication, then traps
  hv::write(
    &mut memory,
    0,
    &bytes(&[
      0xd4000002, 0xd2a80004, 0xf2860004, 0xc8dffc85, 0xb4ffffe5, 0xd4000002, 0x14000000,
    ]),
  )?;
  // secondary publishes its entry context and mpidr, then calls cpu_off
  hv::write(
    &mut memory,
    0x100,
    &bytes(&[
      0xd2a80001, 0xf2860001, 0xf9000420, 0xd53800a2, 0xf9000822, 0xc89ffc20, 0x52800040,
      0x72b08000, 0xd4000002, 0x14000000,
    ]),
  )?;
  let controller = Arc::new(Mutex::new(power::create(
    &[0, 1],
    std::slice::from_ref(&(0x40000000..0x40010000)),
  )?));
  let mut primary = hv::cpu(&vm)?;
  hv::affinity(&mut primary, 0)?;
  hv::enter(&mut primary, 0x40000000)?;
  let context = 0x12345678abcdef42;
  for (register, value) in [(0, 0xc4000003), (1, 1), (2, 0x40000100), (3, context)] {
    hv::set(&mut primary, register, value)?;
  }
  let factory = hv::factory(&vm);
  std::thread::scope(|scope| -> anyhow::Result<()> {
    let (start, receiver) = mpsc::sync_channel::<(usize, u64, u64)>(1);
    let (ready, initialized) = mpsc::sync_channel(1);
    let secondary = controller.clone();
    let worker = scope.spawn(move || -> anyhow::Result<()> {
      let mut cpu = hv::create_cpu(factory)?;
      hv::affinity(&mut cpu, 1)?;
      ready.send(())?;
      let (target, entry, context) = receiver.recv()?;
      ensure!(target == 1, "Startup was sent to the wrong CPU owner");
      hv::enter(&mut cpu, entry)?;
      hv::set(&mut cpu, 0, context)?;
      power::started(&mut *lock(&secondary)?, target)?;
      hv::bounded(&mut cpu, Duration::from_secs(2), hypercall)?;
      let command = hv::get(&cpu, 0)? as u32;
      ensure!(
        power::call(&mut *lock(&secondary)?, target, command, [0; 3]) == Reply::CpuOff,
        "Secondary did not request CPU_OFF"
      );
      power::stopped(&mut *lock(&secondary)?, target)?;
      ready.send(())?;
      // retain the parked vcpu until the primary has also stopped executing
      receiver.recv()?;
      Ok(())
    });
    initialized.recv_timeout(Duration::from_secs(5))?;
    hv::bounded(&mut primary, Duration::from_secs(2), hypercall)?;
    let command = hv::get(&primary, 0)? as u32;
    let args = [
      hv::get(&primary, 1)?,
      hv::get(&primary, 2)?,
      hv::get(&primary, 3)?,
    ];
    let Reply::CpuOn {
      target,
      entry,
      context,
    } = power::call(&mut *lock(&controller)?, 0, command, args)
    else {
      anyhow::bail!("Primary did not request a valid CPU_ON");
    };
    start.send((target, entry, context))?;
    hv::set(&mut primary, 0, 0)?;
    hv::bounded(&mut primary, Duration::from_secs(2), hypercall)?;
    ensure!(
      hv::get(&primary, 0)? == 0,
      "CPU_ON success was not returned to the guest"
    );
    initialized.recv_timeout(Duration::from_secs(5))?;
    start.send((1, 0, 0))?;
    worker
      .join()
      .map_err(|_| anyhow::anyhow!("Secondary CPU owner panicked"))??;
    Ok(())
  })?;
  let mut data = [0; 24];
  hv::read(&memory, 0x3000, &mut data)?;
  ensure!(
    u64::from_le_bytes(data[..8].try_into()?) == context,
    "Secondary did not publish readiness"
  );
  ensure!(
    u64::from_le_bytes(data[8..16].try_into()?) == context,
    "Startup context was not preserved"
  );
  ensure!(
    u64::from_le_bytes(data[16..].try_into()?) & 0xff00ffffff == 1,
    "Wrong secondary affinity"
  );
  ensure!(
    power::call(&mut *lock(&controller)?, 0, 0xc4000004, [1, 0, 0]) == Reply::Value(1),
    "CPU_OFF was not committed after parking"
  );
  println!("Native guest CPU_ON, entry context, secondary execution and CPU_OFF passed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
