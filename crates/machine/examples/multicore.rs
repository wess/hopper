#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::{arm, hypervisor as hv};
  use std::{
    sync::{mpsc, Arc, Barrier},
    time::Duration,
  };

  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x10000)?;
  // read mpidr, publish identity/ready, acquire the peer's ready flag, then hvc
  let program = [
    0xd53800a0u32,
    0xd2800022,
    0xf9000420,
    0xc89ffc22,
    0xc8dffc64,
    0xb4ffffe4,
    0xd4000002,
  ]
  .into_iter()
  .flat_map(u32::to_le_bytes)
  .collect::<Vec<_>>();
  hv::write(&mut memory, 0, &program)?;
  let factory = hv::factory(&vm);
  let identities = std::thread::scope(|scope| -> anyhow::Result<Vec<u64>> {
    let (ready, receiver) = mpsc::sync_channel(2);
    let mut workers = Vec::new();
    let mut starts = Vec::new();
    let stopped = Arc::new(Barrier::new(2));
    for index in 0..2u64 {
      let ready = ready.clone();
      let stopped = stopped.clone();
      let (start, receiver) = mpsc::sync_channel(1);
      starts.push(start);
      workers.push(scope.spawn(move || -> anyhow::Result<u64> {
        let mut cpu = hv::create_cpu(factory)?;
        hv::affinity(&mut cpu, index)?;
        hv::enter(&mut cpu, 0x40000000)?;
        hv::set(&mut cpu, 1, 0x40002000 + index * 0x1000)?;
        hv::set(&mut cpu, 3, 0x40002000 + (1 - index) * 0x1000)?;
        ready.send(())?;
        drop(ready);
        ensure!(receiver.recv()?, "Peer CPU did not initialize");
        let affinity = hv::bounded(&mut cpu, Duration::from_secs(2), |cpu| {
          let hv::Exit::Exception { syndrome, .. } = hv::run(cpu)? else {
            anyhow::bail!("CPU {index} did not complete the guest handshake");
          };
          ensure!(
            arm::decode(syndrome) == arm::Trap::Hypercall(0),
            "Unexpected CPU {index} trap"
          );
          let affinity = hv::get(cpu, 0)?;
          ensure!(
            affinity & 0xff00ffffff == index,
            "CPU affinity does not match its identity"
          );
          Ok(affinity)
        });
        stopped.wait();
        affinity
      }));
    }
    drop(ready);
    let initialized = (0..2).all(|_| receiver.recv_timeout(Duration::from_secs(5)).is_ok());
    for start in starts {
      let _ = start.send(initialized);
    }
    let results: Vec<_> = workers
      .into_iter()
      .map(|worker| {
        worker
          .join()
          .map_err(|_| anyhow::anyhow!("CPU owner thread panicked"))?
      })
      .collect();
    results.into_iter().collect()
  })?;
  for (index, affinity) in identities.iter().enumerate() {
    let mut state = [0; 16];
    hv::read(&memory, 0x2000 + index * 0x1000, &mut state)?;
    ensure!(
      u64::from_le_bytes(state[..8].try_into()?) == 1,
      "CPU readiness was not published"
    );
    ensure!(
      u64::from_le_bytes(state[8..].try_into()?) == *affinity,
      "Shared RAM lost the CPU identity"
    );
  }
  ensure!(identities.len() == 2, "Missing guest CPU");
  println!("Two native CPU owner threads completed a release/acquire guest RAM handshake");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
