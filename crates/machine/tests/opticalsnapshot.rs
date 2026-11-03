#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod support;
use machine::devices::virtio::{pci, queue, scsi};
use std::fs;
use support::{descriptor, Ram, BASE};

#[path = "storage/opticalrecord.rs"]
mod record;

fn private() -> tempfile::TempDir {
  use std::os::unix::fs::PermissionsExt;
  tempfile::Builder::new()
    .permissions(fs::Permissions::from_mode(0o700))
    .tempdir()
    .unwrap()
}

fn reopen(saved: scsi::snapshot::State) -> scsi::snapshot::State {
  let root = private();
  let directory = fs::File::open(root.path()).unwrap();
  let expected = scsi::snapshot::record::sectors(&saved);
  let bytes = scsi::snapshot::record::save(&saved, &directory, || Ok(())).unwrap();
  drop(saved);
  scsi::snapshot::record::load(&directory, &bytes, expected, || Ok(())).unwrap()
}

fn packet(media: &scsi::Optical, cdb: &[u8], incoming: u32) -> (Ram, queue::Queue, queue::Chain) {
  let config = scsi::config(media);
  let cdb_size = u32::from_le_bytes(config[24..28].try_into().unwrap());
  let sense_size = u32::from_le_bytes(config[20..24].try_into().unwrap());
  let mut ram = Ram::default();
  ram.0[0x1000] = 1;
  ram.0[0x1013..0x1013 + cdb.len()].copy_from_slice(cdb);
  descriptor(&mut ram, 0, BASE + 0x1000, 19 + cdb_size, 1, 1);
  descriptor(&mut ram, 1, BASE + 0x2000, 12 + sense_size, 3, 2);
  descriptor(&mut ram, 2, BASE + 0x3000, incoming, 2, 0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  let mut queue = queue::create(&ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap();
  let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
  (ram, queue, chain)
}

fn command(media: &mut scsi::Optical, cdb: &[u8], incoming: u32) -> Ram {
  let (mut ram, mut queue, chain) = packet(media, cdb, incoming);
  let written = scsi::execute(media, &mut ram, &chain, 2).unwrap();
  queue::complete(&mut queue, &mut ram, &chain, written).unwrap();
  ram
}

fn same_stats(a: &scsi::Stats, b: &scsi::Stats) {
  assert_eq!(a.commands, b.commands);
  assert_eq!(a.check_conditions, b.check_conditions);
  assert_eq!(a.queues, b.queues);
  assert_eq!(a.last_missing, b.last_missing);
  assert_eq!(a.last_control, b.last_control);
  assert_eq!(a.last_transfer, b.last_transfer);
  assert_eq!((a.cdb_size, a.sense_size), (b.cdb_size, b.sense_size));
}

#[test]
fn media_state_survives_source_replacement_reset_and_destruction() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("installer");
  fs::write(&path, [0x5a; 4096]).unwrap();
  let mut original = scsi::open(&path).unwrap();
  scsi::configure(&mut original, 20, 4, 18);
  scsi::configure(&mut original, 24, 4, 16);
  let (mut ram, _, chain) = packet(&original, &[0x25], 8);
  ram.0[0x1001] = 2;
  scsi::execute(&mut original, &mut ram, &chain, 2).unwrap();
  let (mut ram, _, mut chain) = packet(&original, &[0x25], 8);
  chain.buffers[0].span.length += 1;
  scsi::execute(&mut original, &mut ram, &chain, 2).unwrap();
  let (mut ram, _, mut chain) = packet(&original, &[], 0);
  ram.0[0x1000..0x1018].fill(0);
  ram.0[0x1004] = 5;
  ram.0[0x1008] = 1;
  chain.buffers[0].span.length = 24;
  scsi::execute(&mut original, &mut ram, &chain, 0).unwrap();
  command(&mut original, &[0x1b, 0, 0, 0, 2, 0], 0);
  assert_eq!(command(&mut original, &[0], 0).0[0x200a], 2);
  let config = scsi::config(&original);
  let stats = scsi::stats(&original);
  let saved = reopen(scsi::snapshot::capture(&original).unwrap());
  fs::rename(&path, root.path().join("retired")).unwrap();
  fs::write(&path, [0xe1; 4096]).unwrap();
  fs::write(root.path().join("retired"), [0xa1; 4096]).unwrap();
  scsi::reset(&mut original);
  drop(original);
  fs::remove_file(root.path().join("retired")).unwrap();
  let target = scsi::open(&path).unwrap();
  for _ in 0..2 {
    let mut restored = scsi::snapshot::restore(&saved, &target).unwrap();
    assert_eq!(scsi::config(&restored), config);
    same_stats(&scsi::stats(&restored), &stats);
    let sense = command(&mut restored, &[3, 0, 0, 0, 18, 0], 18);
    assert_eq!((sense.0[0x3002], sense.0[0x300c]), (2, 0x3a));
    let sense = command(&mut restored, &[3, 0, 0, 0, 18, 0], 18);
    assert_eq!((sense.0[0x3002], sense.0[0x300c]), (0, 0));
    assert_eq!(command(&mut restored, &[0], 0).0[0x200a], 2);
    command(&mut restored, &[0x1b, 0, 0, 0, 3, 0], 0);
    let read = command(&mut restored, &[0x28, 0, 0, 0, 0, 1, 0, 0, 1, 0], 2048);
    assert_eq!(&read.0[0x3000..0x3800], &[0x5a; 2048]);
    let write = command(&mut restored, &[0x2a], 0);
    assert_eq!(
      (write.0[0x200a], write.0[0x200e], write.0[0x2018]),
      (2, 7, 0x27)
    );
  }
  assert_eq!(fs::read(&path).unwrap(), [0xe1; 4096]);
  fs::write(root.path().join("smaller"), [0; 2048]).unwrap();
  let smaller = scsi::open(&root.path().join("smaller")).unwrap();
  assert!(scsi::snapshot::restore(&saved, &smaller).is_err());
}

#[test]
fn pci_recovery_restores_negotiation_and_rejects_changed_capacity_before_mutation() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("media");
  fs::write(&path, [0; 4096]).unwrap();
  let mut original = pci::optical(scsi::open(&path).unwrap()).unwrap();
  let mut ram = Ram::default();
  pci::write(&mut original, &mut ram, pci::SPECIFIC + 20, 4, 18).unwrap();
  pci::write(&mut original, &mut ram, pci::SPECIFIC + 24, 4, 16).unwrap();
  let saved = pci::snapshot::capture(&original).unwrap();
  let transport = pci::snapshot::wire::encode(&saved).unwrap();
  drop(original);
  let stage = private();
  let directory = fs::File::open(stage.path()).unwrap();
  let bytes = pci::snapshot::storagewire::save(&saved, &directory, || Ok(())).unwrap();
  drop(saved);
  let mut target = pci::optical(scsi::open(&path).unwrap()).unwrap();
  let saved = pci::snapshot::records::decode(
    &transport,
    pci::snapshot::records::Contents::Image {
      directory: &directory,
      bytes: &bytes,
    },
    &target,
    || Ok(()),
  )
  .unwrap();
  pci::snapshot::restore(&saved, &mut target, &ram).unwrap();
  assert_eq!(pci::read(&mut target, pci::SPECIFIC + 20, 4).unwrap(), 18);
  assert_eq!(pci::read(&mut target, pci::SPECIFIC + 24, 4).unwrap(), 16);
  fs::write(&path, [0; 2048]).unwrap();
  let mut smaller = pci::optical(scsi::open(&path).unwrap()).unwrap();
  assert!(pci::snapshot::restore(&saved, &mut smaller, &ram).is_err());
  assert_eq!(pci::read(&mut smaller, pci::SPECIFIC + 20, 4).unwrap(), 96);
  assert_eq!(pci::read(&mut smaller, 20, 1).unwrap(), 0);
  assert_eq!(pci::completed(&smaller), 0);
  assert!(!smaller.pci.bus_master());
}
