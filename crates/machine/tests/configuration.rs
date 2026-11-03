use machine::{
  devices::virtio::{block, console, gpu, input, pci, scsi},
  runtime::Boot,
  snapshot::configuration::fingerprint,
};
use std::{fs::OpenOptions, time::Duration};

const ID: &str = "01010101-0101-0101-0101-010101010101";

fn boot() -> Boot {
  Boot {
    firmware: vec![0x14; 131073],
    variables: vec![0xff],
    memory: 1 << 30,
    cpus: 2,
    timeout: Duration::from_secs(10),
  }
}

fn devices() -> anyhow::Result<[Option<pci::Device>; 7]> {
  Ok([
    None,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    None,
    None,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ])
}

#[test]
fn compatibility_changes_bind_identity_firmware_topology_and_each_device() -> anyhow::Result<()> {
  let mut boot = boot();
  let mut hardware = devices()?;
  let baseline = fingerprint(ID, &boot, &hardware, || Ok(()))?;
  assert_eq!(baseline, fingerprint(ID, &boot, &devices()?, || Ok(()))?);
  assert_ne!(
    baseline,
    fingerprint(
      "02020202-0202-0202-0202-020202020202",
      &boot,
      &hardware,
      || Ok(())
    )?
  );
  boot.firmware[65536] ^= 1;
  assert_ne!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  boot.firmware[65536] ^= 1;
  boot.memory *= 2;
  assert_ne!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  boot.memory /= 2;
  boot.cpus = 1;
  assert_ne!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  boot.cpus = 2;
  for (slot, replacement) in [
    (1, Some(pci::graphics(gpu::create(800, 600)?)?)),
    (
      2,
      Some(pci::controller(input::create(input::Kind::Tablet))?),
    ),
    (6, Some(pci::serial(console::create("org.hopper.other")?)?)),
    (6, None),
  ] {
    let original = std::mem::replace(&mut hardware[slot], replacement);
    assert_ne!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
    hardware[slot] = original;
  }
  hardware.swap(2, 3);
  assert_ne!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  hardware.swap(2, 3);
  let original = std::mem::replace(
    &mut hardware[1].as_mut().unwrap().pci,
    machine::devices::pci::Function::new(1, 2, 3, 4)?,
  );
  assert_ne!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  hardware[1].as_mut().unwrap().pci = original;
  let function = &mut hardware[1].as_mut().unwrap().pci;
  function.write(4, 2, 6)?;
  function.write(0x10, 4, 0xffff_ffff)?;
  function.write(0x3c, 1, 35)?;
  let mut capability = function.read(0x34, 1)? as usize;
  let mut masked = false;
  while capability != 0 {
    if function.read(capability, 1)? == 0x11 {
      function.write(capability + 3, 1, 0xc0)?;
      masked = true;
      break;
    }
    capability = function.read(capability + 1, 1)? as usize;
  }
  assert!(masked);
  assert_eq!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  boot.variables = vec![7; 65536];
  boot.timeout = Duration::from_secs(20);
  assert_eq!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  let mut controller = input::create(input::Kind::Keyboard);
  input::configure(&mut controller, 0, 1, 1);
  input::enqueue(
    &mut controller,
    &[
      input::Event {
        kind: 1,
        code: 30,
        value: 1,
      },
      input::SYN,
    ],
  )?;
  hardware[2] = Some(pci::controller(controller)?);
  assert_eq!(baseline, fingerprint(ID, &boot, &hardware, || Ok(()))?);
  Ok(())
}

#[test]
fn disk_and_media_geometry_bind_but_captured_contents_and_writeback_do_not() -> anyhow::Result<()> {
  let root = tempfile::tempdir()?;
  let path = root.path().join("disk");
  std::fs::write(&path, vec![0; 4096])?;
  let mut hardware = devices()?;
  let make = |readonly, id| -> anyhow::Result<_> { pci::create(block::open(&path, readonly, id)?) };
  hardware[4] = Some(make(false, [b'a'; 20])?);
  let baseline = fingerprint(ID, &boot(), &hardware, || Ok(()))?;
  hardware[4] = Some(make(true, [b'a'; 20])?);
  assert_ne!(baseline, fingerprint(ID, &boot(), &hardware, || Ok(()))?);
  hardware[4] = Some(make(false, [b'b'; 20])?);
  assert_ne!(baseline, fingerprint(ID, &boot(), &hardware, || Ok(()))?);
  std::fs::write(&path, vec![7; 4096])?;
  let mut disk = block::open(&path, false, [b'a'; 20])?;
  block::writeback(&mut disk, true)?;
  hardware[4] = Some(pci::create(disk)?);
  assert_eq!(baseline, fingerprint(ID, &boot(), &hardware, || Ok(()))?);
  OpenOptions::new().write(true).open(&path)?.set_len(8192)?;
  hardware[4] = Some(make(false, [b'a'; 20])?);
  assert_ne!(baseline, fingerprint(ID, &boot(), &hardware, || Ok(()))?);
  hardware[0] = Some(pci::optical(scsi::open(&path)?)?);
  let media = fingerprint(ID, &boot(), &hardware, || Ok(()))?;
  std::fs::write(&path, vec![8; 8192])?;
  hardware[0] = Some(pci::optical(scsi::open(&path)?)?);
  assert_eq!(media, fingerprint(ID, &boot(), &hardware, || Ok(()))?);
  OpenOptions::new().write(true).open(&path)?.set_len(16384)?;
  hardware[0] = Some(pci::optical(scsi::open(&path)?)?);
  assert_ne!(media, fingerprint(ID, &boot(), &hardware, || Ok(()))?);
  Ok(())
}

#[test]
fn authorization_brackets_chunks_devices_and_final_publication() -> anyhow::Result<()> {
  let hardware = devices()?;
  let mut checks = 0;
  fingerprint(ID, &boot(), &hardware, || {
    checks += 1;
    Ok(())
  })?;
  assert!(checks > 20);
  for deny in 0..checks {
    let mut index = 0;
    assert!(fingerprint(ID, &boot(), &hardware, || {
      anyhow::ensure!(index != deny, "revoked");
      index += 1;
      Ok(())
    })
    .unwrap_err()
    .to_string()
    .contains("revoked"));
  }
  assert!(fingerprint("", &boot(), &hardware, || Ok(())).is_err());
  let mut invalid = boot();
  invalid.cpus = 3;
  assert!(fingerprint(ID, &invalid, &hardware, || Ok(())).is_err());
  Ok(())
}
