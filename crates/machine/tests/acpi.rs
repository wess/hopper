use machine::{acpi, platform::Topology};

fn topology(cpus: u32) -> Topology {
  Topology {
    memory: 0x10000000,
    cpus,
    distributor_size: 0x10000,
    redistributor_size: 0x2000000,
  }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
  u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn sum(bytes: &[u8]) -> u8 {
  bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

fn table(blob: &[u8], address: u64) -> &[u8] {
  let start = (address - acpi::BASE) as usize;
  assert_eq!(start % 8, 0);
  let end = start + u32_at(blob, start + 4) as usize;
  assert!(end <= blob.len());
  let bytes = &blob[start..end];
  assert_eq!(sum(bytes), 0);
  bytes
}

#[test]
fn root_addresses_lengths_and_checksums_form_a_complete_handoff() {
  let blob = acpi::bundle(&topology(1)).unwrap();
  assert_eq!(blob.len(), acpi::SIZE);
  assert_eq!(&blob[..8], b"RSD PTR ");
  assert_eq!(sum(&blob[..20]), 0);
  assert_eq!(sum(&blob[..36]), 0);
  assert_eq!(blob[15], 2);
  assert_eq!(u32_at(&blob, 20), 36);
  assert_eq!(u32_at(&blob, 16), 0);
  let xsdt = table(&blob, u64_at(&blob, 24));
  assert_eq!(&xsdt[..4], b"XSDT");
  assert_eq!(xsdt.len(), 60);
  let fadt = table(&blob, u64_at(xsdt, 36));
  assert_eq!(&fadt[..4], b"FACP");
  assert_eq!(fadt.len(), 276);
  assert_eq!(u32_at(fadt, 112), 1 << 20);
  assert_eq!(&fadt[129..131], &3u16.to_le_bytes());
  let dsdt = table(&blob, u64_at(fadt, 140));
  assert_eq!(&dsdt[..4], b"DSDT");
  assert!(dsdt.windows(9).any(|bytes| bytes == b"ACPI0007\0"));
  assert_eq!(&table(&blob, u64_at(xsdt, 44))[..4], b"APIC");
  assert_eq!(&table(&blob, u64_at(xsdt, 52))[..4], b"GTDT");
}

#[test]
fn gic_cpu_identity_and_timer_interrupts_match_the_device_tree() {
  let blob = acpi::bundle(&topology(3)).unwrap();
  let xsdt = table(&blob, u64_at(&blob, 24));
  let madt = table(&blob, u64_at(xsdt, 44));
  for index in 0..3 {
    let cpu = 44 + index * 80;
    assert_eq!(&madt[cpu..cpu + 2], &[11, 80]);
    assert_eq!(u32_at(madt, cpu + 8), index as u32);
    assert_eq!(u64_at(madt, cpu + 68), index as u64);
    assert_eq!(u32_at(madt, cpu + 12), 1);
  }
  let gic = 44 + 3 * 80;
  assert_eq!(u64_at(madt, gic + 8), machine::platform::DISTRIBUTOR);
  assert_eq!(madt[gic + 20], 3);
  assert_eq!(u64_at(madt, gic + 24 + 4), machine::platform::REDISTRIBUTOR);
  assert_eq!(u32_at(madt, gic + 24 + 12), 0x2000000);
  let timer = table(&blob, u64_at(xsdt, 52));
  for (offset, irq) in [(48, 29), (56, 30), (64, 27), (72, 26)] {
    assert_eq!(u32_at(timer, offset), irq);
    assert_eq!(u32_at(timer, offset + 4), 4);
  }
}

#[test]
fn maximum_topology_fits_and_invalid_regions_are_rejected() {
  assert_eq!(acpi::bundle(&topology(256)).unwrap().len(), acpi::SIZE);
  assert!(acpi::bundle(&topology(0)).is_err());
  assert!(acpi::bundle(&topology(257)).is_err());
  let mut input = topology(1);
  input.redistributor_size = u64::MAX;
  assert!(acpi::bundle(&input).is_err());
}
