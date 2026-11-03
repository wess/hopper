use anyhow::{ensure, Context};
use engine::machines::{
  native::{
    assets, config,
    restore::{Phase, Record},
  },
  Machines,
};
use machine::{
  devices::virtio::{block, console, gpu, input, pci},
  runtime::{self, variables},
  snapshot::{
    configuration,
    manifest::{Binding, Entry, Layout, Manifest},
  },
};
use sha2::{Digest, Sha256};
use std::{
  fs::{self, File, OpenOptions, Permissions},
  os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
  path::Path,
  time::Duration,
};

fn private(path: &Path) -> anyhow::Result<()> {
  fs::create_dir(path)?;
  fs::set_permissions(path, Permissions::from_mode(0o700))?;
  Ok(())
}
fn sealed(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
  fs::write(path, bytes)?;
  fs::set_permissions(path, Permissions::from_mode(0o400))?;
  Ok(())
}
fn identity(path: &Path, bytes: &mut Vec<u8>) -> anyhow::Result<()> {
  let metadata = fs::metadata(path)?;
  bytes.extend_from_slice(&metadata.dev().to_le_bytes());
  bytes.extend_from_slice(&metadata.ino().to_le_bytes());
  Ok(())
}

pub fn prepare(root: &Path, id: &str, worker: &Path) -> anyhow::Result<Machines> {
  let manager = Machines {
    root: root.join("machines"),
  };
  let machine = model::Machine {
    id: id.into(),
    name: "Synthetic configured generation".into(),
    guest: model::GuestOs::Windows,
    profile: "windows".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 1,
      disk_gib: 64,
    },
    agent_access: true,
    agent_generation: 0,
    runtime: Some(model::MachineRuntime::Hypervisor),
    installer: None,
  };
  store::json::write(
    &manager.root.join("records").join(format!("{id}.json")),
    &machine,
  )?;
  let paths = config::initialize(&manager, id)?;
  let firmware = root.join("firmware");
  private(&firmware)?;
  // bounded busy loop followed by PSCI_SYSTEM_RESET; no OS or captured execution state.
  let code: [u32; 7] = [
    0x52a20001, 0x71000421, 0x54ffffe1, 0x52800120, 0x72b08000, 0xd4000002, 0x14000000,
  ];
  let code: Vec<_> = code.iter().flat_map(|word| word.to_le_bytes()).collect();
  fs::write(firmware.join("windows.fd"), &code)?;
  fs::write(firmware.join("variables.fd"), [0xff])?;
  fs::write(
    firmware.join("manifest.json"),
    serde_json::to_vec(&serde_json::json!({
      "size": code.len(), "sha256": format!("{:x}", Sha256::digest(&code)),
      "variables": {"size": 1, "sha256": format!("{:x}", Sha256::digest([0xff]))},
    }))?,
  )?;
  assets::verify(worker, &firmware)?;
  let generation = uuid::Uuid::new_v4();
  private(&paths.root.join("generations"))?;
  let current = paths
    .root
    .join("generations")
    .join(generation.simple().to_string());
  private(&current)?;
  private(&current.join("storage4"))?;
  let disk = current.join("storage4/disk");
  let file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(&disk)?;
  file.set_len(64 << 30)?;
  file.sync_all()?;
  drop(file);
  drop(variables::create(&current.join("variables"), &[0x7f])?);
  let digest = format!(
    "{:x}",
    Sha256::digest(uuid::Uuid::parse_str(id)?.as_bytes())
  );
  let disk_id: [u8; 20] = format!("hpr{}", &digest[..17]).as_bytes().try_into()?;
  let hardware = [
    None,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    Some(pci::create(block::open(&disk, false, disk_id)?)?),
    None,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let boot = runtime::Boot {
    firmware: code,
    variables: vec![0x7f],
    memory: 1 << 30,
    cpus: 2,
    timeout: Duration::MAX,
  };
  let configuration = configuration::fingerprint(id, &boot, &hardware, || Ok(()))?;
  drop(hardware);
  let selected = Record {
    machine: model::native::machine_identity(id).context("Invalid synthetic identity")?,
    snapshot: *uuid::Uuid::new_v4().as_bytes(),
    generation: *generation.as_bytes(),
    configuration,
    rollback: *uuid::Uuid::new_v4().as_bytes(),
    previous: None,
    phase: Phase::Active,
  };
  let layout = Layout {
    cpus: 2,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, 1 << 30),
    ],
    devices: [0, 2, 3, 3, 1, 0, 5],
  };
  // metadata fixture only; these receipts never authorize checkpoint replay.
  let entries = layout
    .records()?
    .into_iter()
    .map(|record| Entry {
      record,
      length: 36,
      checksum: [8; 32],
    })
    .collect();
  let manifest = Manifest::new(
    Binding {
      machine: selected.machine,
      snapshot: selected.snapshot,
      configuration,
    },
    layout,
    entries,
  )?;
  let mut receipt = Vec::from(b"HGR1");
  receipt.extend_from_slice(&selected.generation);
  receipt.extend_from_slice(&manifest.encode());
  receipt.extend_from_slice(&Sha256::digest(&receipt));
  sealed(&current.join("generation.hgr"), &receipt)?;
  let mut marker = Vec::from(b"HGA1");
  marker.extend_from_slice(&Sha256::digest(&receipt));
  marker.extend_from_slice(&[7; 16]);
  marker.extend_from_slice(&Sha256::digest(&marker));
  sealed(&current.join("attempt.hga"), &marker)?;
  let mut ownership = Vec::from(b"HGO1");
  ownership.extend_from_slice(&Sha256::digest(&receipt));
  identity(&current, &mut ownership)?;
  identity(&current.join("variables"), &mut ownership)?;
  for slot in 0..7 {
    if slot == 4 {
      ownership.push(1);
      identity(&current.join("storage4"), &mut ownership)?;
      identity(&disk, &mut ownership)?;
      ownership.extend_from_slice(&(64u64 << 30).to_le_bytes());
    } else {
      ownership.extend_from_slice(&[0; 41]);
    }
  }
  ownership.extend_from_slice(&Sha256::digest(&ownership));
  ensure!(
    ownership.len() == 387,
    "Incorrect synthetic ownership bound"
  );
  sealed(&current.join("ownership.hgo"), &ownership)?;
  sealed(&paths.root.join("restore.hrr"), &selected.encode()?)?;
  let installation = paths.root.join("installation");
  fs::write(
    &installation,
    serde_json::to_vec(
      &serde_json::json!({"version": 1, "vmId": id, "installation": {"state": "systemStarted"}}),
    )?,
  )?;
  fs::set_permissions(&installation, Permissions::from_mode(0o600))?;
  File::open(&paths.root)?.sync_all()?;
  Ok(manager)
}
