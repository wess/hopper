//! Firmware-visible topology of Hopper's initial ARM64 platform.

use anyhow::ensure;
use vm_fdt::FdtWriter;

pub const RAM: u64 = 0x40000000;
pub const UART: u64 = 0x09000000;
pub const DISTRIBUTOR: u64 = 0x08000000;
pub const REDISTRIBUTOR: u64 = 0x0a000000;
pub const ECAM: u64 = 0x10000000;
pub const PCI_MEMORY: u64 = 0x20000000;
pub const PCI_MEMORY_SIZE: u64 = 0x10000000;
pub const MSI: u64 = 0x30000000;

#[derive(Clone, Copy)]
pub struct Msi {
  pub size: u64,
  pub first: u32,
  pub count: u32,
}

pub struct Topology {
  pub memory: u64,
  pub cpus: u32,
  pub distributor_size: u64,
  pub redistributor_size: u64,
  pub msi: Option<Msi>,
}

pub fn validate(topology: &Topology) -> anyhow::Result<()> {
  ensure!(
    topology.memory >= 0x1000000 && RAM.checked_add(topology.memory).is_some(),
    "Guest memory range is invalid"
  );
  ensure!(
    topology.cpus > 0 && topology.cpus <= 256,
    "Guest CPU count is invalid"
  );
  ensure!(
    topology.distributor_size > 0 && topology.distributor_size <= UART - DISTRIBUTOR,
    "Interrupt distributor overlaps the console"
  );
  ensure!(
    topology.redistributor_size >= topology.cpus as u64 * 0x20000
      && topology.redistributor_size <= 0x10000000 - REDISTRIBUTOR,
    "Interrupt redistributor region is invalid"
  );
  if let Some(msi) = topology.msi {
    ensure!(
      msi.size >= 0x1000
        && msi.size <= RAM - MSI
        && msi.first >= crate::devices::pci::FIRST_IRQ + 4
        && msi.count > 0
        && msi
          .first
          .checked_add(msi.count)
          .is_some_and(|end| end <= 1020),
      "MSI frame or interrupt range is invalid"
    );
  }
  Ok(())
}

pub fn tree(topology: &Topology) -> anyhow::Result<Vec<u8>> {
  validate(topology)?;
  let mut fdt = FdtWriter::new()?;
  let root = fdt.begin_node("")?;
  fdt.property_string("compatible", "hopper,arm64")?;
  fdt.property_u32("#address-cells", 2)?;
  fdt.property_u32("#size-cells", 2)?;
  fdt.property_u32("interrupt-parent", 1)?;
  let memory = fdt.begin_node("memory@40000000")?;
  fdt.property_string("device_type", "memory")?;
  fdt.property_array_u64("reg", &[RAM, topology.memory])?;
  fdt.end_node(memory)?;
  let cpus = fdt.begin_node("cpus")?;
  fdt.property_u32("#address-cells", 1)?;
  fdt.property_u32("#size-cells", 0)?;
  for index in 0..topology.cpus {
    let cpu = fdt.begin_node(&format!("cpu@{index:x}"))?;
    fdt.property_string("device_type", "cpu")?;
    fdt.property_string("compatible", "arm,arm-v8")?;
    fdt.property_string("enable-method", "psci")?;
    fdt.property_u32("reg", index)?;
    fdt.end_node(cpu)?;
  }
  fdt.end_node(cpus)?;
  let gic = fdt.begin_node("interrupt-controller@8000000")?;
  fdt.property_string("compatible", "arm,gic-v3")?;
  fdt.property_null("interrupt-controller")?;
  fdt.property_u32("#interrupt-cells", 3)?;
  fdt.property_u32("#address-cells", 2)?;
  fdt.property_u32("#size-cells", 2)?;
  fdt.property_null("ranges")?;
  fdt.property_u32("phandle", 1)?;
  fdt.property_array_u64(
    "reg",
    &[
      DISTRIBUTOR,
      topology.distributor_size,
      REDISTRIBUTOR,
      topology.redistributor_size,
    ],
  )?;
  if let Some(msi) = topology.msi {
    let frame = fdt.begin_node("v2m@30000000")?;
    fdt.property_string("compatible", "arm,gic-v2m-frame")?;
    fdt.property_null("msi-controller")?;
    fdt.property_u32("phandle", 3)?;
    fdt.property_array_u64("reg", &[MSI, msi.size])?;
    fdt.property_u32("arm,msi-base-spi", msi.first)?;
    fdt.property_u32("arm,msi-num-spis", msi.count)?;
    fdt.end_node(frame)?;
  }
  fdt.end_node(gic)?;
  let timer = fdt.begin_node("timer")?;
  fdt.property_string("compatible", "arm,armv8-timer")?;
  fdt.property_array_u32("interrupts", &[1, 13, 4, 1, 14, 4, 1, 11, 4, 1, 10, 4])?;
  fdt.end_node(timer)?;
  let psci = fdt.begin_node("psci")?;
  fdt.property_string_list(
    "compatible",
    vec!["arm,psci-1.0".into(), "arm,psci-0.2".into()],
  )?;
  fdt.property_string("method", "hvc")?;
  fdt.end_node(psci)?;
  let clock = fdt.begin_node("clock")?;
  fdt.property_string("compatible", "fixed-clock")?;
  fdt.property_u32("#clock-cells", 0)?;
  fdt.property_u32("clock-frequency", 24000000)?;
  fdt.property_u32("phandle", 2)?;
  fdt.end_node(clock)?;
  let uart = fdt.begin_node("uart@9000000")?;
  fdt.property_string_list(
    "compatible",
    vec!["arm,pl011".into(), "arm,primecell".into()],
  )?;
  fdt.property_array_u64("reg", &[UART, 0x1000])?;
  fdt.property_array_u32("clocks", &[2, 2])?;
  fdt.property_string_list("clock-names", vec!["uartclk".into(), "apb_pclk".into()])?;
  fdt.end_node(uart)?;
  let flash = fdt.begin_node("flash@0")?;
  fdt.property_string("compatible", "cfi-flash")?;
  fdt.property_array_u64("reg", &[0, 0x4000000, 0x4000000, 0x4000000])?;
  fdt.property_u32("bank-width", 4)?;
  fdt.end_node(flash)?;
  let pci = fdt.begin_node("pci@10000000")?;
  fdt.property_string("compatible", "pci-host-ecam-generic")?;
  fdt.property_string("device_type", "pci")?;
  fdt.property_u32("#address-cells", 3)?;
  fdt.property_u32("#size-cells", 2)?;
  fdt.property_u32("#interrupt-cells", 1)?;
  if topology.msi.is_some() {
    fdt.property_u32("msi-parent", 3)?;
  }
  fdt.property_array_u64("reg", &[ECAM, crate::devices::pci::ECAM_SIZE])?;
  fdt.property_array_u32("bus-range", &[0, 0])?;
  fdt.property_array_u32(
    "ranges",
    &[
      0x02000000,
      0,
      PCI_MEMORY as u32,
      0,
      PCI_MEMORY as u32,
      0,
      PCI_MEMORY_SIZE as u32,
    ],
  )?;
  fdt.property_array_u32("interrupt-map-mask", &[0xf800, 0, 0, 7])?;
  let mut routes = Vec::new();
  for device in 0..32u8 {
    for pin in 1..=4u8 {
      routes.extend([
        (device as u32) << 11,
        0,
        0,
        pin as u32,
        1,
        0,
        0,
        0,
        crate::devices::pci::interrupt(device, pin)? - 32,
        4,
      ]);
    }
  }
  fdt.property_array_u32("interrupt-map", &routes)?;
  fdt.end_node(pci)?;
  let chosen = fdt.begin_node("chosen")?;
  fdt.property_string("stdout-path", "/uart@9000000")?;
  fdt.end_node(chosen)?;
  fdt.end_node(root)?;
  Ok(fdt.finish()?)
}
