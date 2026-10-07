use machine::platform::{tree, Topology};

fn topology() -> Topology {
  Topology {
    memory: 0x10000000,
    cpus: 1,
    distributor_size: 0x10000,
    redistributor_size: 0x800000,
    msi: None,
  }
}

#[test]
fn device_tree_has_a_valid_blob_header() {
  let bytes = tree(&topology()).unwrap();
  assert_eq!(&bytes[..4], &0xd00dfeedu32.to_be_bytes());
  assert_eq!(
    u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize,
    bytes.len()
  );
}

#[test]
fn invalid_memory_and_interrupt_regions_are_rejected() {
  let mut input = topology();
  input.memory = u64::MAX;
  assert!(tree(&input).is_err());
  input = topology();
  input.distributor_size = 0x1000001;
  assert!(tree(&input).is_err());
  input = topology();
  input.redistributor_size = 0x10000;
  assert!(tree(&input).is_err());
  input = topology();
  input.cpus = 0;
  assert!(tree(&input).is_err());
}
