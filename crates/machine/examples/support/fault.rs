use anyhow::bail;
use machine::{hypervisor as hv, platform};

pub fn inspect(cpu: &hv::Cpu<'_>, ram: &hv::Memory<'_>, size: u64) -> anyhow::Result<()> {
  let pc = hv::get(cpu, 31)?;
  let fault = hv::fault(cpu)?;
  if pc
    .checked_sub(fault.vector)
    .is_some_and(|offset| offset < 0x800 && offset.is_multiple_of(0x80))
  {
    let mut instruction = [0; 4];
    if let Some(offset) = pc
      .checked_sub(platform::RAM)
      .filter(|offset| offset.checked_add(4).is_some_and(|end| end <= size))
    {
      hv::read(ram, offset as usize, &mut instruction)?;
      if u32::from_le_bytes(instruction) == 0x14000000 {
        bail!("Installer stopped in exception vector at PC 0x{pc:x}; faulting instruction 0x{:x}, syndrome 0x{:x}, address 0x{:x}", fault.instruction, fault.syndrome, fault.address);
      }
    }
  }
  Ok(())
}

pub fn report(cpu: &hv::Cpu<'_>) -> anyhow::Result<()> {
  let fault = hv::fault(cpu)?;
  let pc = hv::get(cpu, 31)?;
  let level = (hv::get(cpu, 34)? >> 2) & 3;
  eprintln!("Installer probe ended at PC 0x{pc:x}, exception level {level}");
  if pc
    .checked_sub(fault.vector)
    .is_some_and(|offset| offset < 0x800)
  {
    eprintln!("Installer last exception: return address 0x{:x}, syndrome 0x{:x}, address 0x{:x}", fault.instruction, fault.syndrome, fault.address);
  }
  Ok(())
}
