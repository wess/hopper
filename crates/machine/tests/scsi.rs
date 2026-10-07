#[path = "support/disk.rs"]
mod disk;
mod support;
use machine::{
  devices::virtio::{pci, queue, scsi},
  dma::{self, Span},
};
use support::{descriptor, Ram, BASE};

fn request(cdb: &[u8], incoming: u32) -> (Ram, queue::Queue, queue::Chain) {
  let mut ram = Ram::default();
  let mut header = [0; 51];
  header[0] = 1;
  header[19..19 + cdb.len()].copy_from_slice(cdb);
  ram.0[0x1000..0x101e].copy_from_slice(&header[..30]);
  ram.0[0x1100..0x1115].copy_from_slice(&header[30..]);
  for (index, offset, length, flags, next) in [
    (0, 0x1000, 30, 1, 1),
    (1, 0x1100, 21, 1, 2),
    (2, 0x2000, 31, 3, 3),
    (3, 0x2100, 77, 3, 4),
    (4, 0x3000, incoming, 2, 0),
  ] {
    descriptor(&mut ram, index, BASE + offset, length, flags, next);
  }
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  let mut queue = queue::create(&ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap();
  let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
  (ram, queue, chain)
}

fn response(ram: &Ram) -> Vec<u8> {
  let mut bytes = vec![0; 108];
  dma::read(
    ram,
    &[
      Span {
        address: BASE + 0x2000,
        length: 31,
      },
      Span {
        address: BASE + 0x2100,
        length: 77,
      },
    ],
    0,
    &mut bytes,
  )
  .unwrap();
  bytes
}

#[test]
fn optical_reads_real_sectors_through_fragmented_request_and_response_buffers() {
  let data: Vec<_> = (0..8192).map(|byte| (byte / 2048) as u8).collect();
  let image = disk::image(&data);
  let mut media = scsi::open(&image.0).unwrap();
  let (mut ram, mut queue, chain) = request(&[0x28, 0, 0, 0, 0, 1, 0, 0, 2, 0], 4096);
  let used = scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  queue::complete(&mut queue, &mut ram, &chain, used).unwrap();
  assert_eq!(used, 108 + 4096);
  assert_eq!(response(&ram), vec![0; 108]);
  assert_eq!(&ram.0[0x3000..0x4000], &data[2048..6144]);
  assert_eq!(std::fs::read(&image.0).unwrap(), data);
}

#[test]
fn write_protection_and_request_sense_preserve_the_installer() {
  let image = disk::image(&[0x5a; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  let (mut ram, _, chain) = request(&[0x2a], 0);
  assert_eq!(scsi::execute(&mut media, &mut ram, &chain, 2).unwrap(), 108);
  let result = response(&ram);
  assert_eq!(
    (result[10], result[11], result[14], result[24]),
    (2, 0, 7, 0x27)
  );
  let (mut ram, _, chain) = request(&[3, 0, 0, 0, 18, 0], 18);
  assert_eq!(scsi::execute(&mut media, &mut ram, &chain, 2).unwrap(), 126);
  assert_eq!((ram.0[0x3002], ram.0[0x300c]), (7, 0x27));
  assert_eq!(std::fs::read(&image.0).unwrap(), vec![0x5a; 4096]);
}

#[test]
fn invalid_targets_overruns_and_bidirectional_requests_have_transport_errors() {
  let image = disk::image(&[0x5a; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  let (mut ram, _, chain) = request(&[0x25], 8);
  ram.0[0x1001] = 1;
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  assert_eq!(response(&ram)[11], 3);
  let (mut ram, _, chain) = request(&[0x28, 0, 0, 0, 0, 0, 0, 0, 2, 0], 2048);
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  assert_eq!(response(&ram)[11], 1);
  assert_eq!(&ram.0[0x3000..0x3800], &[0; 2048]);
  let (mut ram, _, mut chain) = request(&[0x25], 8);
  chain.buffers[1].span.length += 1;
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  assert_eq!(response(&ram)[11], 9);
  chain.buffers[0].writable = true;
  assert!(scsi::execute(&mut media, &mut ram, &chain, 2).is_err());
}

#[test]
fn capacity_inquiry_and_toc_describe_readonly_removable_media() {
  let image = disk::image(&[0; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  for (cdb, expected) in [
    (vec![0x25], vec![0, 0, 0, 1, 0, 0, 8, 0]),
    (vec![0x12, 0, 0, 0, 36, 0], vec![5, 0x80, 5, 2, 31]),
    (
      vec![0x43, 0, 0, 0, 0, 0, 0, 0, 20, 0],
      vec![0, 18, 1, 1, 0, 0x14, 1, 0],
    ),
  ] {
    let (mut ram, _, chain) = request(&cdb, 36);
    scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
    assert_eq!(response(&ram)[10..12], [0, 0]);
    assert_eq!(&ram.0[0x3000..0x3000 + expected.len()], &expected);
  }
  let mut device = pci::optical(media).unwrap();
  assert_eq!(device.pci.read(2, 2).unwrap(), 0x1048);
  assert_eq!(pci::features(&device), pci::VERSION);
  assert_eq!(pci::read(&mut device, 18, 2).unwrap(), 3);
}

#[test]
fn inclusive_and_older_count_based_scans_both_discover_only_the_optical_lun() {
  let image = disk::image(&[0; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  let hint = u32::from_le_bytes(scsi::config(&media)[32..36].try_into().unwrap());
  for inclusive in [false, true] {
    let mut found = Vec::new();
    for lun in 0..hint + u32::from(inclusive) {
      let (mut ram, _, chain) = request(&[0x12, 0, 0, 0, 36, 0], 36);
      ram.0[0x1003] = lun as u8;
      scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
      if response(&ram)[11] == 0 {
        assert_eq!(response(&ram)[10], 0);
        assert_eq!(ram.0[0x3000], 5);
        found.push(lun);
      } else {
        assert_eq!(response(&ram)[11], 3);
      }
    }
    assert_eq!(found, [0]);
  }
}

#[test]
fn media_bounds_and_configuration_reset_are_checked() {
  let image = disk::image(&[0; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  scsi::configure(&mut media, 20, 4, 18);
  scsi::configure(&mut media, 24, 4, 16);
  scsi::configure(&mut media, 24, 4, u32::MAX);
  assert_eq!(scsi::config(&media)[20..28], [18, 0, 0, 0, 16, 0, 0, 0]);
  scsi::reset(&mut media);
  assert_eq!(scsi::config(&media)[20..28], [96, 0, 0, 0, 32, 0, 0, 0]);
  assert_eq!(scsi::config(&media)[28..36], [0, 0, 7, 0, 1, 0, 0, 0]);
  for cdb in [
    vec![0x28, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 1, 0],
    vec![0xa8, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff],
  ] {
    let (mut ram, _, chain) = request(&cdb, 0);
    scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
    assert_eq!(response(&ram)[10], 2);
  }
  let bad = disk::image(&[0; 512]);
  assert!(scsi::open(&bad.0).is_err());
}

#[test]
fn optical_feature_queries_and_media_removal_report_actual_state() {
  let image = disk::image(&[0; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  let (mut ram, _, chain) = request(&[0x46, 2, 0, 0x10, 0, 0, 0, 0, 20, 0], 20);
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  assert_eq!(
    &ram.0[0x3006..0x3014],
    &[0, 0x10, 0, 0x10, 1, 8, 0, 0, 8, 0, 0, 1, 0, 0]
  );
  let (mut ram, _, chain) = request(&[0x1b, 0, 0, 0, 2, 0], 0);
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  let (mut ram, _, chain) = request(&[0], 0);
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  assert_eq!(
    (response(&ram)[10], response(&ram)[14], response(&ram)[24]),
    (2, 2, 0x3a)
  );
  let (mut ram, _, chain) = request(&[0x46, 0, 0, 0, 0, 0, 0, 0, 64, 0], 64);
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  assert_eq!(&ram.0[0x3006..0x3008], &[0, 0]);
  let (mut ram, _, chain) = request(&[0x1b, 0, 0, 0, 3, 0], 0);
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  let (mut ram, _, chain) = request(&[0], 0);
  scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
  assert_eq!(response(&ram)[10..12], [0, 0]);
}

#[test]
fn control_requests_complete_without_consuming_event_buffers() {
  let image = disk::image(&[0; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  let (mut ram, _, mut chain) = request(&[], 1);
  ram.0[0x1000..0x1018].fill(0);
  ram.0[0x1008] = 1;
  assert_eq!(scsi::execute(&mut media, &mut ram, &chain, 0).unwrap(), 1);
  assert_eq!(response(&ram)[0], 0);
  assert!(scsi::execute(&mut media, &mut ram, &chain, 1).is_err());
  ram.0[0x1000] = 4;
  scsi::execute(&mut media, &mut ram, &chain, 0).unwrap();
  assert_eq!(response(&ram)[0], 9);
  chain.buffers.truncate(1);
  assert!(scsi::execute(&mut media, &mut ram, &chain, 0).is_err());
}

#[test]
fn negotiated_six_byte_commands_work_and_truncated_long_commands_return_sense() {
  let image = disk::image(&[0; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  scsi::configure(&mut media, 24, 4, 6);
  for (cdb, valid) in [(vec![0x1a, 0, 0x2a, 0, 24, 0], true), (vec![0x25], false)] {
    let (mut ram, _, mut chain) = request(&cdb, 24);
    chain.buffers[0].span.length = 25;
    chain.buffers.remove(1);
    scsi::execute(&mut media, &mut ram, &chain, 2).unwrap();
    if valid {
      assert_eq!(&ram.0[0x3000..0x3007], &[23, 0, 0x80, 0, 0x2a, 18, 8]);
      assert_eq!(response(&ram)[10], 0);
    } else {
      assert_eq!((response(&ram)[10], response(&ram)[24]), (2, 0x24));
    }
  }
}

#[test]
fn asynchronous_queries_report_no_supported_events_without_overrunning_buffers() {
  let image = disk::image(&[0; 4096]);
  let mut media = scsi::open(&image.0).unwrap();
  let (mut ram, _, mut chain) = request(&[], 0);
  ram.0[0x1000..0x1010].fill(0);
  ram.0[0x1000] = 1;
  ram.0[0x1004] = 1;
  ram.0[0x100c] = 0x10;
  chain.buffers[0].span.length = 16;
  chain.buffers.remove(1);
  assert_eq!(scsi::execute(&mut media, &mut ram, &chain, 0).unwrap(), 5);
  assert_eq!(&response(&ram)[..5], &[0; 5]);
  ram.0[0x1005] = 1;
  scsi::execute(&mut media, &mut ram, &chain, 0).unwrap();
  assert_eq!(response(&ram)[4], 3);
  chain.buffers.truncate(1);
  assert!(scsi::execute(&mut media, &mut ram, &chain, 0).is_err());
}
