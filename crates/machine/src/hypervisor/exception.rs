use super::{check, ffi, Cpu};

pub struct Fault {
  pub vector: u64,
  pub instruction: u64,
  pub syndrome: u64,
  pub address: u64,
}

pub fn fault(cpu: &Cpu<'_>) -> anyhow::Result<Fault> {
  let read = |register| -> anyhow::Result<u64> {
    let mut value = 0;
    check(
      unsafe { ffi::hv_vcpu_get_sys_reg(cpu.id, register, &mut value) },
      "Read guest exception state",
    )?;
    Ok(value)
  };
  Ok(Fault {
    vector: read(0xc600)?,
    instruction: read(0xc201)?,
    syndrome: read(0xc290)?,
    address: read(0xc300)?,
  })
}
