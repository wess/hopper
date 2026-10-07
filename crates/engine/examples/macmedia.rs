#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use engine::machines::{
    vz::{self, records, Service},
    Actor, Machines,
  };
  let root = tempfile::tempdir()?;
  let image = root.path().join("invalid.ipsw");
  std::fs::write(&image, b"owned malformed restore image")?;
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Restore inspection diagnostic".into(),
      profile: "macos".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      agent_access: true,
      installer: Some(image.to_str().unwrap().into()),
    },
  )?;
  let (client, _) = vz::channel();
  let service = Service::new(manager.clone(), client);
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .worker_threads(1)
    .build()?;
  for attempt in 0..2 {
    let error = if attempt == 0 {
      let (progress, _) = tokio::sync::watch::channel(vz::mac::Phase::Inspecting);
      runtime
        .block_on(service.prepare_mac(&machine.id, Actor::Person, progress))
        .err()
    } else {
      runtime
        .block_on(service.inspect_mac(&machine.id, Actor::Person))
        .err()
    }
    .ok_or_else(|| anyhow::anyhow!("Malformed restore image was accepted"))?;
    ensure!(
      error
        .to_string()
        .contains("macOS restore inspection failed"),
      "Unexpected native inspection rejection: {error}"
    );
    ensure!(
      std::fs::read(&image)? == b"owned malformed restore image",
      "Inspection changed source media"
    );
    // completion can release retained media after publishing the result.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::fs::read_dir(manager.root.join("vz"))?.count() != 0 {
      ensure!(
        std::time::Instant::now() < deadline,
        "Completed inspection retained staging data"
      );
      std::thread::sleep(std::time::Duration::from_millis(10));
    }
  }
  manager.set_agent_access(&machine.id, false)?;
  ensure!(
    runtime
      .block_on(service.inspect_mac(&machine.id, Actor::Agent))
      .is_err(),
    "Revoked inspection was accepted"
  );
  ensure!(
    std::fs::read_dir(manager.root.join("vz"))?.count() == 0,
    "Revoked inspection staged media"
  );
  println!("Signed native macOS launch and SDK inspection rejected malformed media, preserved the source, released staging state and rejected revoked-agent access; no valid IPSW, installation or macOS boot was verified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native restore inspection requires Apple silicon macOS")
}
