use super::*;
use snapshot::records::{self, Contents};
use std::{fs, io::Read};

#[test]
fn foreign_backend_bindings_and_payload_types_reject_without_target_mutation() {
  let target = device();
  let saved = snapshot::capture(&target).unwrap();
  let transport = snapshot::wire::encode(&saved).unwrap();
  drop(saved);
  let tablet = input::wire::encode(&input::create(Kind::Tablet)).unwrap();
  assert!(records::decode(&transport, Contents::Bytes(&tablet), &target, || Ok(())).is_err());
  struct Unread;
  impl Read for Unread {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
      panic!("foreign payload was read");
    }
  }
  assert!(
    records::decode(&transport, Contents::Graphics(&mut Unread), &target, || Ok(
      ()
    ))
    .is_err()
  );
  let console = pci::serial(console::create("org.hopper.setup").unwrap()).unwrap();
  assert!(records::decode(
    &transport,
    Contents::Graphics(&mut Unread),
    &console,
    || Ok(())
  )
  .is_err());
  let mut target = target;
  assert_eq!(pci::completed(&target), 0);
  assert_eq!(pci::read(&mut target, 20, 1).unwrap(), 0);
  assert!(!target.pci.bus_master());
  let saved = snapshot::capture(&console).unwrap();
  let transport = snapshot::wire::encode(&saved).unwrap();
  drop(saved);
  let foreign = console::wire::encode(&console::create("org.hopper.other").unwrap()).unwrap();
  assert!(records::decode(&transport, Contents::Bytes(&foreign), &console, || Ok(())).is_err());
}

#[test]
fn final_policy_revocation_discards_fully_decoded_records() {
  let target = device();
  let saved = snapshot::capture(&target).unwrap();
  let transport = snapshot::wire::encode(&saved).unwrap();
  let bytes = snapshot::backendwire::encode(&saved).unwrap().unwrap();
  drop(saved);
  let mut checks = 0;
  records::decode(&transport, Contents::Bytes(&bytes), &target, || {
    checks += 1;
    Ok(())
  })
  .unwrap();
  for stop in 1..=checks {
    let mut calls = 0;
    assert!(
      records::decode(&transport, Contents::Bytes(&bytes), &target, || {
        calls += 1;
        anyhow::ensure!(calls < stop, "revoked");
        Ok(())
      })
      .is_err()
    );
  }
}

#[test]
fn complete_input_device_recovers_after_original_process_exit() {
  use std::os::unix::fs::PermissionsExt;
  let root = tempfile::Builder::new()
    .permissions(fs::Permissions::from_mode(0o700))
    .tempdir()
    .unwrap();
  for phase in ["save", "load", "load"] {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
      .args([
        "--exact",
        "records::process_child",
        "--ignored",
        "--nocapture",
      ])
      .env("HOPPER_INPUT_RECORD_PHASE", phase)
      .env("HOPPER_INPUT_RECORD_ROOT", root.path())
      .output()
      .unwrap();
    assert!(
      result.status.success(),
      "child {phase}: {}{}",
      String::from_utf8_lossy(&result.stdout),
      String::from_utf8_lossy(&result.stderr)
    );
  }
}

#[test]
#[ignore = "invoked by separate-process input device test"]
fn process_child() {
  let phase = std::env::var("HOPPER_INPUT_RECORD_PHASE").unwrap();
  let root = std::path::PathBuf::from(std::env::var_os("HOPPER_INPUT_RECORD_ROOT").unwrap());
  if phase == "save" {
    let mut source = device();
    let mut ram = Ram::default();
    initialize(&mut source, &mut ram);
    ram.0[0x102..0x104].copy_from_slice(&2u16.to_le_bytes());
    for code in [30, 42] {
      pci::send_input(
        &mut source,
        &mut ram,
        &[
          Event {
            kind: 1,
            code,
            value: 1,
          },
          SYN,
        ],
      )
      .unwrap();
    }
    let saved = snapshot::capture(&source).unwrap();
    save(
      &root.join("transport"),
      &snapshot::wire::encode(&saved).unwrap(),
    );
    save(
      &root.join("backend"),
      &snapshot::backendwire::encode(&saved).unwrap().unwrap(),
    );
    save(&root.join("ram"), &ram.0);
    fs::File::open(&root).unwrap().sync_all().unwrap();
    drop((saved, source, ram));
  } else {
    assert_eq!(phase, "load");
    let transport = fs::read(root.join("transport")).unwrap();
    let backend = fs::read(root.join("backend")).unwrap();
    let mut ram = Ram(fs::read(root.join("ram")).unwrap());
    assert_eq!(ram.0.len(), 65536);
    let mut target = device();
    let saved = records::decode(&transport, Contents::Bytes(&backend), &target, || Ok(())).unwrap();
    snapshot::restore(&saved, &mut target, &ram).unwrap();
    assert_eq!(pci::completed(&target), 2);
    assert_eq!(pci::read(&mut target, pci::ISR, 1).unwrap(), 1);
    assert_eq!(target.pci.memory(0x20000000), Some((0, 0)));
    ram.0[0x102..0x104].copy_from_slice(&4u16.to_le_bytes());
    pci::write(&mut target, &mut ram, pci::NOTIFY, 2, 0).unwrap();
    assert_eq!(&ram.0[0x1010..0x1018], &[1, 0, 42, 0, 1, 0, 0, 0]);
    pci::release_input(&mut target, &mut ram).unwrap();
    ram.0[0x102..0x104].copy_from_slice(&7u16.to_le_bytes());
    pci::write(&mut target, &mut ram, pci::NOTIFY, 2, 0).unwrap();
    assert_eq!(pci::completed(&target), 7);
    assert_eq!(&ram.0[0x1020..0x1028], &[1, 0, 30, 0, 0, 0, 0, 0]);
    assert_eq!(&ram.0[0x1028..0x1030], &[1, 0, 42, 0, 0, 0, 0, 0]);
  }
}

fn save(path: &std::path::Path, bytes: &[u8]) {
  use std::{io::Write, os::unix::fs::OpenOptionsExt};
  let mut file = fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(path)
    .unwrap();
  file.write_all(bytes).unwrap();
  file.sync_all().unwrap();
}
