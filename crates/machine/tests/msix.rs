use machine::devices::{pci::Function, virtio::pci::msix};

#[test]
fn capability_chain_preserves_readonly_size_and_writable_masks() {
  let mut function = Function::new(0x1af4, 0x1042, 0x010000, 1).unwrap();
  function.add_bar(2, 0x1000, false).unwrap();
  let first = function.add_vendor_capability(&[9, 0, 4, 1]).unwrap();
  let vector = function.add_msix(3, 2, 0, 0x800).unwrap();
  let last = function.add_vendor_capability(&[9, 0, 4, 2]).unwrap();
  assert_eq!(function.read(first + 1, 1).unwrap(), vector as u32);
  assert_eq!(function.read(vector + 1, 1).unwrap(), last as u32);
  assert_eq!(function.read(last + 1, 1).unwrap(), 0);
  assert_eq!(function.read(vector + 2, 2).unwrap(), 2);
  assert_eq!(function.read(vector + 4, 4).unwrap(), 2);
  assert_eq!(function.read(vector + 8, 4).unwrap(), 0x802);
  function.write(vector, 4, u32::MAX).unwrap();
  assert_eq!(
    function.read(vector, 4).unwrap(),
    0xc0020000 | (last as u32) << 8 | 0x11
  );
  assert!(function.msix_enabled() && function.msix_masked());
  function.write(vector + 3, 1, 0x80).unwrap();
  assert!(function.msix_enabled() && !function.msix_masked());
  function.write(vector + 2, 2, 0).unwrap();
  assert!(!function.msix_enabled());
  assert_eq!(function.read(vector + 2, 2).unwrap(), 2);
  assert!(function.add_msix(3, 2, 0, 0x800).is_err());
}

#[test]
fn masked_messages_coalesce_and_flush_using_current_table_values() {
  let mut state = msix::create(64).unwrap();
  for vector in [0, 63] {
    msix::raise(&mut state, vector);
  }
  msix::raise(&mut state, 63);
  msix::raise(&mut state, 64);
  msix::raise(&mut state, u16::MAX);
  assert_eq!(
    msix::read(&state, msix::PENDING, 8).unwrap(),
    0x8000000000000001
  );
  msix::write(&mut state, msix::PENDING, 8, 0).unwrap();
  assert_eq!(
    msix::read(&state, msix::PENDING + 4, 4).unwrap(),
    0x80000000
  );
  msix::write(&mut state, 63 * 16, 8, 0x1234567830000040).unwrap();
  msix::write(&mut state, 63 * 16 + 8, 4, 95).unwrap();
  msix::write(&mut state, 63 * 16 + 12, 4, 0xfffffffe).unwrap();
  assert_eq!(msix::read(&state, 63 * 16 + 12, 4).unwrap(), 0);
  assert!(msix::take(&mut state, false, false).is_empty());
  assert!(msix::take(&mut state, true, true).is_empty());
  assert_eq!(
    msix::take(&mut state, true, false),
    vec![msix::Message {
      address: 0x1234567830000040,
      data: 95,
    }]
  );
  assert_eq!(msix::read(&state, msix::PENDING, 8).unwrap(), 1);
  assert!(msix::take(&mut state, true, false).is_empty());
  msix::clear(&mut state);
  assert_eq!(msix::read(&state, msix::PENDING, 8).unwrap(), 0);
}

#[test]
fn invalid_tables_and_accesses_fail_without_modifying_pending_bits() {
  assert!(msix::create(0).is_err());
  assert!(msix::create(65).is_err());
  let mut state = msix::create(2).unwrap();
  msix::raise(&mut state, 1);
  for (offset, width) in [(1, 4), (0, 3), (0x1000, 4), (u64::MAX, 8)] {
    assert!(msix::read(&state, offset, width).is_err());
    assert!(msix::write(&mut state, offset, width, u64::MAX).is_err());
  }
  msix::write(&mut state, 0x400, 4, u64::MAX).unwrap();
  assert_eq!(msix::read(&state, 0x400, 4).unwrap(), 0);
  assert_eq!(msix::read(&state, msix::PENDING, 4).unwrap(), 2);
  for (count, table, pending) in [
    (0, 0, 0x800),
    (2049, 0, 0x800),
    (2, 4, 0x800),
    (2, 0, 0),
    (2, 0x1000, 0x800),
  ] {
    let mut function = Function::new(0x1af4, 0x1042, 0x010000, 1).unwrap();
    function.add_bar(2, 0x1000, false).unwrap();
    assert!(function.add_msix(count, 2, table, pending).is_err());
    assert_eq!(function.read(0x34, 1).unwrap(), 0);
  }
}
