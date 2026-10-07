use super::Reason;
use crate::{
  arm, debug,
  devices::{
    flash, pci as config, serial,
    virtio::{bus, pci},
  },
  hypervisor::{self as hv, registers as regs},
  platform, psci, smccc,
};
use anyhow::{bail, ensure, Context};
use std::{
  sync::{mpsc::SyncSender, Arc, Mutex},
  time::Instant,
};

pub struct State<'a, 'vm> {
  pub devices: &'a mut [Option<pci::Device>; 7],
  pub ram: &'a mut hv::Memory<'vm>,
  pub nvram: &'a mut hv::Memory<'vm>,
  pub variables: &'a mut flash::Flash,
  pub gic: &'a hv::gic::Gic<'vm>,
  pub power: Arc<Mutex<psci::power::Power>>,
  pub launch: SyncSender<hv::secondary::Boot>,
  pub cpus: u32,
  pub debug: Vec<debug::State>,
  pub console: serial::Console,
  pub tail: Vec<u8>,
  pub enter: Option<Instant>,
}

#[derive(Default)]
pub struct Action {
  pub off: bool,
  pub reason: Option<Reason>,
}

pub fn interrupts(state: &mut State<'_, '_>) -> anyhow::Result<()> {
  for (slot, device) in state.devices.iter_mut().enumerate() {
    if let Some(device) = device {
      hv::gic::signal(
        state.gic,
        config::interrupt(slot as u8, 1)?,
        pci::interrupt(device),
      )?;
      for message in pci::messages(device) {
        hv::gic::message(state.gic, message.address, message.data)?;
      }
    }
  }
  Ok(())
}

fn advance(registers: &mut regs::Registers) -> anyhow::Result<()> {
  let pc = regs::read(registers, 31)?;
  regs::write(
    registers,
    31,
    pc.checked_add(4).context("Guest PC overflow")?,
  )
}

fn value(registers: &regs::Registers, register: u8) -> anyhow::Result<u64> {
  if register == 31 {
    Ok(0)
  } else {
    regs::read(registers, register.into())
  }
}

fn read(registers: &mut regs::Registers, register: u8, value: u64) -> anyhow::Result<()> {
  if register != 31 {
    regs::write(registers, register.into(), value)?;
  }
  Ok(())
}

pub fn dispatch(
  state: &mut State<'_, '_>,
  caller: usize,
  exit: hv::Exit,
  registers: &mut regs::Registers,
) -> anyhow::Result<Action> {
  let mut action = Action::default();
  let hv::Exit::Exception {
    syndrome,
    physical_address,
    ..
  } = exit
  else {
    bail!("Unhandled native guest exit: {exit:?}");
  };
  match arm::decode(syndrome) {
    arm::Trap::DataAbort(Some(access)) => {
      let address = physical_address;
      if (platform::UART..platform::UART + 0x1000).contains(&address) {
        ensure!(access.bytes <= 4, "Unsupported serial access width");
        let offset = address - platform::UART;
        if access.write {
          if let Some(byte) = serial::write(
            &mut state.console,
            offset,
            value(registers, access.register)? as u32,
          ) {
            if state.tail.len() == 64 {
              state.tail.remove(0);
            }
            state.tail.push(byte);
            if state.tail.ends_with(b"cdboot.efi") {
              state.enter = Some(Instant::now() + std::time::Duration::from_secs(1));
            }
          }
        } else {
          read(
            registers,
            access.register,
            serial::read(&mut state.console, offset).into(),
          )?;
        }
      } else if (platform::ECAM..platform::ECAM + config::ECAM_SIZE).contains(&address) {
        let offset = address - platform::ECAM;
        if access.write {
          bus::write(
            state.devices,
            state.ram,
            offset,
            access.bytes.into(),
            value(registers, access.register)? as u32,
          )?;
        } else {
          read(
            registers,
            access.register,
            bus::read(state.devices, offset, access.bytes.into())?.into(),
          )?;
        }
      } else if bus::mapped(state.devices, address)?.is_some() {
        if access.write {
          bus::memory_write(
            state.devices,
            state.ram,
            address,
            access.bytes.into(),
            value(registers, access.register)?,
          )?;
        } else {
          read(
            registers,
            access.register,
            bus::memory_read(state.devices, address, access.bytes.into())?.1,
          )?;
        }
      } else if (0x4000000..0x8000000).contains(&address) {
        let offset = (address - 0x4000000) as usize;
        if access.write {
          ensure!(access.bytes == 4, "NOR command requires a 32-bit access");
          if let Some(changed) = flash::write(
            state.variables,
            offset,
            value(registers, access.register)? as u32,
          )? {
            hv::write(
              state.nvram,
              changed.start,
              &flash::bytes(state.variables)[changed],
            )?;
          }
          hv::protect(
            state.nvram,
            if flash::array(state.variables) { 1 } else { 0 },
          )?;
        } else {
          read(
            registers,
            access.register,
            flash::read(state.variables, offset, access.bytes)?,
          )?;
        }
      } else {
        bail!("Unmapped native guest memory access at 0x{address:x}");
      }
      interrupts(state)?;
      advance(registers)?;
    }
    arm::Trap::SystemRegister(access) => {
      let input = if access.read {
        0
      } else {
        value(registers, access.register)?
      };
      let output = debug::access(&mut state.debug[caller], access, input)
        .context("Unsupported guest system register")?;
      if access.read {
        read(registers, access.register, output)?;
      }
      advance(registers)?;
    }
    arm::Trap::Hypercall(0) => {
      let command = regs::read(registers, 0)? as u32;
      let argument = regs::read(registers, 1)?;
      let mut entropy = |bytes: &mut [u8]| -> anyhow::Result<()> {
        if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
          return Err(std::io::Error::last_os_error()).context("Read system entropy");
        }
        Ok(())
      };
      if let Some(reply) = smccc::call(command, argument, &mut entropy) {
        for (register, value) in reply.into_iter().enumerate() {
          regs::write(registers, register as u32, value)?;
        }
      } else {
        let level = regs::read(registers, 2)?;
        let context = regs::read(registers, 3)?;
        let mut power = state
          .power
          .lock()
          .map_err(|_| anyhow::anyhow!("CPU power lock poisoned"))?;
        let reply = if state.cpus == 1 {
          psci::call(command, argument, level as u32)
        } else {
          psci::power::call(&mut power, caller, command, [argument, level, context])
        };
        match reply {
          psci::Reply::Value(value) => regs::write(registers, 0, value as u64)?,
          psci::Reply::CpuOn {
            target,
            entry,
            context,
          } => {
            ensure!(target == 1, "Unknown secondary CPU owner");
            state.debug[target] = debug::State::default();
            let result = state
              .launch
              .try_send(hv::secondary::Boot { entry, context });
            if result.is_err() {
              psci::power::failed(&mut power, target)?;
            }
            regs::write(
              registers,
              0,
              if result.is_ok() { 0 } else { (-6i64) as u64 },
            )?;
          }
          psci::Reply::CpuOff if caller == 1 => action.off = true,
          psci::Reply::Shutdown => action.reason = Some(Reason::Shutdown),
          psci::Reply::Reset => action.reason = Some(Reason::Reset),
          reply => bail!("Unsupported native CPU power request: {reply:?}"),
        }
      }
    }
    trap => bail!("Unsupported native guest trap: {trap:?}"),
  }
  Ok(action)
}
