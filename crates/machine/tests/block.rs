#[path = "support/disk.rs"]
mod disk;
mod support;
use disk::image;

use machine::devices::virtio::{block, queue};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use support::{descriptor, Ram, BASE};

fn request(kind: u32, sector: u64, length: u32) -> (Ram, queue::Queue, queue::Chain) {
  let mut ram = Ram::default();
  let mut header = [0; 16];
  header[..4].copy_from_slice(&kind.to_le_bytes());
  header[8..].copy_from_slice(&sector.to_le_bytes());
  ram.0[0x1000..0x1008].copy_from_slice(&header[..8]);
  ram.0[0x1100..0x1108].copy_from_slice(&header[8..]);
  descriptor(&mut ram, 0, BASE + 0x1000, 8, 1, 1);
  descriptor(&mut ram, 1, BASE + 0x1100, 8, 1, 2);
  descriptor(
    &mut ram,
    2,
    BASE + 0x2000,
    length,
    if kind == 1 { 1 } else { 3 },
    3,
  );
  descriptor(&mut ram, 3, BASE + 0x4000, 1, 2, 0);
  ram.0[0x4000] = 0xff;
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  let mut queue = queue::create(&ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap();
  let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
  (ram, queue, chain)
}

#[test]
fn real_disk_reads_and_writes_complete_through_guest_queues() {
  let image = image(&[b'a'; 1024]);
  let mut disk = block::open(&image.0, false, [b'd'; 20]).unwrap();
  assert_eq!(block::capacity(&disk), 2);
  assert!(!block::readonly(&disk));
  let (mut ram, mut queue, chain) = request(0, 1, 512);
  let used = block::execute(&mut disk, &mut ram, &chain).unwrap();
  assert_eq!(used, 513);
  assert_eq!(&ram.0[0x2000..0x2200], &[b'a'; 512]);
  assert_eq!(ram.0[0x4000], 0);
  queue::complete(&mut queue, &mut ram, &chain, used).unwrap();
  let (mut ram, mut queue, chain) = request(1, 1, 512);
  ram.0[0x2000..0x2200].fill(b'z');
  let used = block::execute(&mut disk, &mut ram, &chain).unwrap();
  assert_eq!(used, 1);
  assert_eq!(ram.0[0x4000], 0);
  queue::complete(&mut queue, &mut ram, &chain, used).unwrap();
  let actual = fs::read(&image.0).unwrap();
  assert_eq!(&actual[..512], &[b'a'; 512]);
  assert_eq!(&actual[512..], &[b'z'; 512]);
  block::writeback(&mut disk, true).unwrap();
  let (mut ram, _, chain) = request(4, 0, 0);
  assert_eq!(block::execute(&mut disk, &mut ram, &chain).unwrap(), 1);
  assert_eq!(ram.0[0x4000], 0);
  block::writeback(&mut disk, false).unwrap();
}

#[test]
fn read_only_media_and_invalid_ranges_never_change_the_backing_file() {
  let image = image(&[b'a'; 1024]);
  let mut disk = block::open(&image.0, true, [b'd'; 20]).unwrap();
  assert!(block::readonly(&disk));
  for (kind, sector, length) in [(1, 0, 512), (0, 2, 512), (0, u64::MAX, 512), (0, 0, 511)] {
    let (mut ram, _, chain) = request(kind, sector, length);
    assert_eq!(block::execute(&mut disk, &mut ram, &chain).unwrap(), 1);
    assert_eq!(ram.0[0x4000], 1);
    assert_eq!(&ram.0[0x2000..0x2200], &[0; 512]);
  }
  assert_eq!(fs::read(&image.0).unwrap(), vec![b'a'; 1024]);
  let (mut ram, _, chain) = request(4, 0, 0);
  block::execute(&mut disk, &mut ram, &chain).unwrap();
  assert_eq!(ram.0[0x4000], 0);
}

#[test]
fn identity_unsupported_commands_and_host_io_errors_report_guest_status() {
  let image = image(&[b'a'; 1024]);
  let mut disk = block::open(&image.0, false, [b'd'; 20]).unwrap();
  let (mut ram, _, chain) = request(8, 0, 20);
  assert_eq!(block::execute(&mut disk, &mut ram, &chain).unwrap(), 21);
  assert_eq!(&ram.0[0x2000..0x2014], &[b'd'; 20]);
  assert_eq!(ram.0[0x4000], 0);
  let (mut ram, _, chain) = request(99, 0, 0);
  assert_eq!(block::execute(&mut disk, &mut ram, &chain).unwrap(), 1);
  assert_eq!(ram.0[0x4000], 2);
  OpenOptions::new()
    .write(true)
    .open(&image.0)
    .unwrap()
    .set_len(512)
    .unwrap();
  let (mut ram, _, chain) = request(0, 1, 512);
  assert_eq!(block::execute(&mut disk, &mut ram, &chain).unwrap(), 1);
  assert_eq!(ram.0[0x4000], 1);
}

#[test]
fn status_can_share_a_descriptor_with_data() {
  let image = image(&[b'a'; 512]);
  let mut disk = block::open(&image.0, true, [b'd'; 20]).unwrap();
  let (mut ram, _, _) = request(0, 0, 512);
  descriptor(&mut ram, 2, BASE + 0x2000, 513, 2, 0);
  let mut queue = queue::create(&ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap();
  let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
  let used = block::execute(&mut disk, &mut ram, &chain).unwrap();
  assert_eq!(used, 513);
  assert_eq!(&ram.0[0x2000..0x2200], &[b'a'; 512]);
  assert_eq!(ram.0[0x2200], 0);
  queue::complete(&mut queue, &mut ram, &chain, used).unwrap();
}

#[test]
fn malformed_disk_geometry_and_requests_fail_before_io() {
  let image = image(&[0; 513]);
  assert!(block::open(&image.0, false, [b'd'; 20]).is_err());
  fs::write(&image.0, [0; 512]).unwrap();
  assert!(block::open(&image.0, true, [0xff; 20]).is_err());
  let mut disk = block::open(&image.0, true, [b'd'; 20]).unwrap();
  let (mut ram, _, mut chain) = request(0, 0, 512);
  chain.buffers[0].span.length = 0;
  assert!(block::execute(&mut disk, &mut ram, &chain).is_err());
  assert_eq!(ram.0[0x4000], 0xff);
}

#[test]
fn owned_backing_file_survives_path_replacement_without_writing_the_replacement() {
  let original = image(&[b'a'; 512]);
  let replacement = image(&[b'r'; 512]);
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .open(&original.0)
    .unwrap();
  let mut retained = file.try_clone().unwrap();
  let mut disk = block::attach(file, false, [b'd'; 20]).unwrap();
  fs::rename(&replacement.0, &original.0).unwrap();
  let (mut ram, _, chain) = request(1, 0, 512);
  ram.0[0x2000..0x2200].fill(b'z');
  block::execute(&mut disk, &mut ram, &chain).unwrap();
  assert_eq!(ram.0[0x4000], 0);
  assert_eq!(fs::read(&original.0).unwrap(), vec![b'r'; 512]);
  retained.seek(SeekFrom::Start(0)).unwrap();
  let mut bytes = Vec::new();
  retained.read_to_end(&mut bytes).unwrap();
  assert_eq!(bytes, vec![b'z'; 512]);
}
