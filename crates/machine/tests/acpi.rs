use machine::{acpi, platform::Topology};

fn topology(cpus: u32) -> Topology {
  Topology {
    memory: 0x10000000,
    cpus,
    distributor_size: 0x10000,
    redistributor_size: 0x2000000,
    msi: None,
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
  assert_eq!(xsdt.len(), 68);
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
fn pci_configuration_and_apertures_match_the_native_bus() {
  use machine::{devices::pci::ECAM_SIZE, platform};
  let blob = acpi::bundle(&topology(1)).unwrap();
  let xsdt = table(&blob, u64_at(&blob, 24));
  let mcfg = table(&blob, u64_at(xsdt, 60));
  assert_eq!(&mcfg[..4], b"MCFG");
  assert_eq!(mcfg.len(), 60);
  assert_eq!(mcfg[8], 1);
  assert_eq!(&mcfg[36..44], &[0; 8]);
  assert_eq!(u64_at(mcfg, 44), platform::ECAM);
  assert_eq!(&mcfg[52..60], &[0; 8]);
  let fadt = table(&blob, u64_at(xsdt, 36));
  let dsdt = table(&blob, u64_at(fadt, 140));
  for identity in [b"PNP0A08\0", b"PNP0A03\0", b"PNP0C02\0"] {
    assert!(dsdt.windows(identity.len()).any(|bytes| bytes == identity));
  }
  let memory = dsdt
    .windows(4)
    .position(|bytes| bytes == [0x87, 23, 0, 0])
    .unwrap();
  assert_eq!(dsdt[memory + 4], 0x0c);
  assert_eq!(dsdt[memory + 5], 1);
  assert_eq!(u32_at(dsdt, memory + 10), platform::PCI_MEMORY as u32);
  assert_eq!(
    u32_at(dsdt, memory + 14),
    (platform::PCI_MEMORY + platform::PCI_MEMORY_SIZE - 1) as u32
  );
  assert_eq!(u32_at(dsdt, memory + 22), platform::PCI_MEMORY_SIZE as u32);
  let ecam = dsdt
    .windows(4)
    .position(|bytes| bytes == [0x86, 9, 0, 1])
    .unwrap();
  assert_eq!(u32_at(dsdt, ecam + 4), platform::ECAM as u32);
  assert_eq!(u32_at(dsdt, ecam + 8), ECAM_SIZE as u32);
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

#[test]
fn msi_frame_and_reserved_memory_match_the_configured_interrupt_range() {
  let mut input = topology(1);
  input.msi = Some(machine::platform::Msi {
    size: 0x1000,
    first: 64,
    count: 32,
  });
  let blob = acpi::bundle(&input).unwrap();
  let xsdt = table(&blob, u64_at(&blob, 24));
  let madt = table(&blob, u64_at(xsdt, 44));
  let frame = &madt[44 + 80 + 24 + 16..];
  assert_eq!(frame.len(), 24);
  assert_eq!(&frame[..2], &[13, 24]);
  assert_eq!(u64_at(frame, 8), machine::platform::MSI);
  assert_eq!(u32_at(frame, 16), 1);
  assert_eq!(&frame[20..24], &[32, 0, 64, 0]);
  let fadt = table(&blob, u64_at(xsdt, 36));
  let dsdt = table(&blob, u64_at(fadt, 140));
  let expected = [0x86, 9, 0, 1, 0, 0, 0, 0x30, 0, 0x10, 0, 0];
  assert!(dsdt.windows(expected.len()).any(|bytes| bytes == expected));
  let tree = machine::platform::tree(&input).unwrap();
  assert!(tree
    .windows(18)
    .any(|bytes| bytes == b"arm,gic-v2m-frame\0"));
  for (size, first, count) in [
    (0, 64, 32),
    (u64::MAX, 64, 32),
    (0x1000, 33, 32),
    (0x1000, 64, 0),
    (0x1000, 1000, 32),
  ] {
    input.msi = Some(machine::platform::Msi { size, first, count });
    assert!(acpi::bundle(&input).is_err());
    assert!(machine::platform::tree(&input).is_err());
  }
}
