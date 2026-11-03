use super::{context, prepare};
use crate::hypervisor::{self as hv, ffi, state};

mod phase;

#[repr(C)]
struct Timebase {
  numer: u32,
  denom: u32,
}

extern "C" {
  fn mach_timebase_info(info: *mut Timebase) -> i32;
}

#[link(name = "Hypervisor", kind = "framework")]
extern "C" {
  fn hv_gic_set_distributor_reg(register: u16, value: u64) -> i32;
  fn hv_gic_set_redistributor_reg(cpu: u64, register: u32, value: u64) -> i32;
  fn hv_gic_get_redistributor_reg(cpu: u64, register: u32, value: *mut u64) -> i32;
}

fn system(cpu: &hv::Cpu<'_>, register: u16, value: u64) -> anyhow::Result<()> {
  hv::check(
    unsafe { ffi::hv_vcpu_set_sys_reg(cpu.id, register, value) },
    "Configure PMU diagnostic register",
  )
}

#[test]
#[ignore = "requires a signed Hypervisor-entitled test executable"]
fn fault_and_timers() -> anyhow::Result<()> {
  let vm = hv::create()?;
  let gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let code = prepare(&vm, 0x3fff0000)?;
  let token = code.token();
  let mut memory = hv::memory(&vm, 0x40000000, 16384)?;
  // UDF, infinite branch, and a forbidden guest exception handler that writes RAM.
  hv::write(&mut memory, 0, &0u32.to_le_bytes())?;
  hv::write(&mut memory, 4, &0x14000000u32.to_le_bytes())?;
  hv::write(&mut memory, 0xa00, &0xf9000020u32.to_le_bytes())?;
  hv::write(&mut memory, 0xa04, &0xd4000002u32.to_le_bytes())?;
  hv::write(&mut memory, 0xa80, &0xf9000020u32.to_le_bytes())?;
  hv::write(&mut memory, 0xa84, &0xd4000002u32.to_le_bytes())?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::enter(&mut cpu, 0x40000000)?;
  hv::set(&mut cpu, 0, 0x12345678)?;
  hv::set(&mut cpu, 1, 0x40003ff8)?;
  system(&cpu, 0xc600, 0x40000800)?;
  let before = state::capture(&cpu)?;
  let interrupts = hv::gic::state::capture(&gic)?;
  let mut original = vec![0; 16384];
  hv::read(&memory, 0, &mut original)?;
  let rejected: anyhow::Result<()> = context::run(&mut cpu, token.address + 0x800, |cpu| {
    hv::enter(cpu, 0x40000000)?;
    let exit = hv::run(cpu)?;
    let hv::Exit::Exception { syndrome, .. } = exit else {
      anyhow::bail!("Fault did not reach its private vector: {exit:?}");
    };
    anyhow::ensure!(crate::arm::decode(syndrome) == crate::arm::Trap::Hypercall(1));
    anyhow::ensure!(hv::get(cpu, 31)? == token.address + 0xa04);
    anyhow::bail!("Deliberate PMU access fault");
  });
  anyhow::ensure!(rejected.unwrap_err().to_string() == "Deliberate PMU access fault");
  anyhow::ensure!(state::capture(&cpu)? == before, "Fault changed CPU context");
  anyhow::ensure!(hv::gic::state::capture(&gic)?.bytes() == interrupts.bytes());
  let mut after = vec![0; 16384];
  hv::read(&memory, 0, &mut after)?;
  anyhow::ensure!(
    after == original,
    "Fault entered the guest handler or changed RAM"
  );

  let expired = context::run(&mut cpu, token.address + 0x800, |cpu| {
    hv::enter(cpu, 0x40000004)?;
    hv::run(cpu)
  });
  anyhow::ensure!(
    expired.is_err(),
    "Infinite guest code escaped its execution deadline"
  );
  anyhow::ensure!(
    state::capture(&cpu)? == before,
    "Deadline changed CPU context"
  );
  anyhow::ensure!(hv::gic::state::capture(&gic)?.bytes() == interrupts.bytes());
  println!("Private exception-vector fault and bounded infinite-loop cleanup passed");

  unsafe {
    hv::check(
      hv_gic_set_distributor_reg(0, 0x12),
      "Enable interrupt distributor",
    )?;
    for (register, value) in [
      (0x10080, (1 << 27) | (1 << 30)),
      (0x10418, 0x80808080),
      (0x1041c, 0x80808080),
      (0x10100, (1 << 27) | (1 << 30)),
    ] {
      hv::check(
        hv_gic_set_redistributor_reg(cpu.id, register, value),
        "Enable timer interrupt routing",
      )?;
    }
    for (register, value) in [(0xc230, 255), (0xc665, 1), (0xc667, 1)] {
      hv::check(
        ffi::hv_gic_set_icc_reg(cpu.id, register, value),
        "Enable CPU interrupt routing",
      )?;
    }
  }

  // both timer types are enabled; compare future and already-due capture behavior.
  hv::check(
    unsafe { ffi::hv_vcpu_set_vtimer_mask(cpu.id, false) },
    "Enable virtual timer delivery",
  )?;
  let mut offset = 0;
  hv::check(
    unsafe { ffi::hv_vcpu_get_vtimer_offset(cpu.id, &mut offset) },
    "Read diagnostic timer epoch",
  )?;
  for due in [false, true] {
    let now = state::clock::ticks().wrapping_sub(offset);
    let deadline = if due { now - 1 } else { now + (1 << 40) };
    for register in [0xdf12, 0xdf1a] {
      system(&cpu, register, deadline)?;
    }
    for register in [0xdf11, 0xdf19] {
      system(&cpu, register, 1)?;
    }
    let before = state::capture(&cpu)?;
    let interrupts = hv::gic::state::capture(&gic)?;
    let saved = token.capture(&mut cpu)?;
    let mut pending = 0;
    hv::check(
      unsafe { hv_gic_get_redistributor_reg(cpu.id, 0x10200, &mut pending) },
      "Read pending timer interrupts",
    )?;
    println!("Timer interrupts already_due={due}: pending=0x{pending:x}");
    anyhow::ensure!(
      pending & (1 << 30) == if due { 1 << 30 } else { 0 },
      "Physical timer routing was not exercised"
    );
    anyhow::ensure!(
      state::capture(&cpu)? == before,
      "Timer capture changed CPU context"
    );
    anyhow::ensure!(
      hv::gic::state::capture(&gic)?.bytes() == interrupts.bytes(),
      "Timer capture changed interrupt state"
    );
    token.restore(&mut cpu, &saved)?;
    anyhow::ensure!(
      state::capture(&cpu)? == before,
      "Timer restore changed CPU context"
    );
    anyhow::ensure!(
      hv::gic::state::capture(&gic)?.bytes() == interrupts.bytes(),
      "Timer restore changed interrupt state"
    );
    println!(
      "Enabled physical/virtual timers: already_due={due}, PMU access preserves CPU and GIC state"
    );
  }
  let mut timebase = Timebase { numer: 0, denom: 0 };
  anyhow::ensure!(unsafe { mach_timebase_info(&mut timebase) } == 0);
  anyhow::ensure!(timebase.numer != 0 && timebase.denom != 0);
  let ticks = 20_000_000u64 * u64::from(timebase.denom) / u64::from(timebase.numer);
  let deadline = state::clock::ticks().wrapping_sub(offset) + ticks;
  for register in [0xdf12, 0xdf1a] {
    system(&cpu, register, deadline)?;
  }
  // the original guest permits IRQs; temporary execution must still suppress handlers.
  hv::set(&mut cpu, 34, 0x345)?;
  let registers = (0..35)
    .map(|index| hv::get(&cpu, index))
    .collect::<anyhow::Result<Vec<_>>>()?;
  context::run(&mut cpu, token.address + 0x800, |cpu| {
    anyhow::ensure!(state::clock::ticks().wrapping_sub(offset) < deadline);
    std::thread::sleep(std::time::Duration::from_millis(40));
    anyhow::ensure!(state::clock::ticks().wrapping_sub(offset) >= deadline);
    token.access(cpu, 0xdce0, None)?;
    Ok(())
  })?;
  for (index, expected) in registers.into_iter().enumerate() {
    anyhow::ensure!(
      hv::get(&cpu, index as u32)? == expected,
      "Crossing deadline changed guest registers"
    );
  }
  for register in [0xdf12, 0xdf1a] {
    anyhow::ensure!(
      state::read(&cpu, register)? == deadline,
      "Crossing deadline changed timer compare"
    );
  }
  for register in [0xdf11, 0xdf19] {
    anyhow::ensure!(
      state::read(&cpu, register)? & 3 == 1,
      "Crossing deadline changed timer control"
    );
  }
  let mut pending = 0;
  hv::check(
    unsafe { hv_gic_get_redistributor_reg(cpu.id, 0x10200, &mut pending) },
    "Read crossed timer interrupt",
  )?;
  anyhow::ensure!(pending & (1 << 30) != 0, "Crossed timer was not pending");
  hv::read(&memory, 0, &mut after)?;
  anyhow::ensure!(
    after == original,
    "Crossing deadline entered guest IRQ handler"
  );
  println!("Deadline crossing keeps timer compares/controls and original IRQ enable, leaves the due interrupt pending and never enters guest handlers");
  phase::verify(&vm, &mut cpu, token, &memory, &gic, offset)?;
  Ok(())
}
