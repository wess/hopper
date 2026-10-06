//! Firmware-visible topology of Hopper's initial ARM64 platform.

use anyhow::ensure;
use vm_fdt::FdtWriter;

pub const RAM: u64 = 0x40000000;
pub const UART: u64 = 0x09000000;
pub const DISTRIBUTOR: u64 = 0x08000000;
pub const REDISTRIBUTOR: u64 = 0x0a000000;

pub struct Topology {
  pub memory: u64,
  pub cpus: u32,
  pub distributor_size: u64,
  pub redistributor_size: u64,
}

pub fn tree(topology: &Topology) -> anyhow::Result<Vec<u8>> {
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
  let chosen = fdt.begin_node("chosen")?;
  fdt.property_string("stdout-path", "/uart@9000000")?;
  fdt.end_node(chosen)?;
  fdt.end_node(root)?;
  Ok(fdt.finish()?)
}
