#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn run() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{native::snapshot::Policy, Machines};
  use machine::{
    devices::virtio::{console, gpu, input, pci},
    snapshot::{
      checkpoint,
      manifest::{Binding, Layout},
      package::Package,
    },
  };
  use model::native::snapshot::Receipt;
  use std::{fs::File, io::Read, path::PathBuf};

  #[derive(serde::Deserialize)]
  #[serde(deny_unknown_fields)]
  struct Probe {
    machine: String,
    configuration: [u8; 32],
    snapshot: String,
    receipt: Receipt,
  }

  let root = PathBuf::from(
    std::env::args_os()
      .nth(1)
      .context("Expected private fixture root")?,
  );
  let probe: Probe = serde_json::from_reader(File::open(root.join("snapshots.json"))?.take(16385))?;
  let manager = Machines {
    root: root.join("machines"),
  };
  let policy = Policy::agent(manager.clone(), &probe.machine, 17)?;
  let parent = File::open(root.join("snapshots"))?;
  let binding = Binding {
    machine: model::native::machine_identity(&probe.machine).context("Invalid VM identity")?,
    snapshot: model::native::machine_identity(&probe.snapshot)
      .context("Invalid snapshot identity")?,
    configuration: probe.configuration,
  };
  let layout = Layout {
    cpus: 2,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, 0x10000000),
    ],
    devices: [0, 2, 3, 3, 0, 0, 5],
  };
  let mode = std::env::args()
    .nth(2)
    .context("Expected commit/deny diagnostic mode")?;
  if mode == "deny" {
    manager.set_agent_access(&probe.machine, false)?;
    manager.set_agent_access(&probe.machine, true)?;
    ensure!(
      policy
        .publish(&parent, probe.receipt, binding, layout)
        .is_err(),
      "Revoked original policy committed a worker stage"
    );
    ensure!(
      Package::open(&parent, binding, layout, || Ok(())).is_err(),
      "Denied worker stage was published"
    );
    println!("Host rejected a real worker stage after saved-policy off/on");
    return Ok(());
  }
  ensure!(mode == "commit", "Unknown snapshot diagnostic mode");
  let package = policy.publish(&parent, probe.receipt, binding, layout)?;
  let hardware = [
    None,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    None,
    None,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let saved = checkpoint::load(&package, &hardware, || policy.check())?;
  ensure!(
    saved.cpus.len() == 2 && saved.primary.memory.ram.size() == 0x10000000,
    "Worker checkpoint topology differs"
  );
  drop(saved);
  drop(package);
  ensure!(Package::open(&parent, binding, layout, || policy.check()).is_ok());
  println!("Host committed a real worker stage while source hardware remained owned and decoded every checkpoint record");
  Ok(())
}

fn main() -> anyhow::Result<()> {
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  return run();
  #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
  anyhow::bail!("Native snapshot host checks require Apple silicon macOS");
}
