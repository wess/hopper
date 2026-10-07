use super::{put, table};
use crate::platform::{self, Topology};

pub(super) fn fadt(dsdt: u64) -> Vec<u8> {
  let mut body = vec![0; 276 - 36];
  body[45 - 36] = 1;
  put(&mut body, 112 - 36, &(1u32 << 20).to_le_bytes());
  put(&mut body, 129 - 36, &3u16.to_le_bytes());
  body[131 - 36] = 3;
  put(&mut body, 140 - 36, &dsdt.to_le_bytes());
  table(b"FACP", 6, &body)
}

pub(super) fn madt(topology: &Topology) -> Vec<u8> {
  let mut body = vec![0; 8];
  for index in 0..topology.cpus {
    let mut cpu = vec![0; 80];
    cpu[0] = 11;
    cpu[1] = 80;
    put(&mut cpu, 4, &index.to_le_bytes());
    put(&mut cpu, 8, &index.to_le_bytes());
    put(&mut cpu, 12, &1u32.to_le_bytes());
    put(&mut cpu, 68, &(index as u64).to_le_bytes());
    body.extend(cpu);
  }
  let mut distributor = [0; 24];
  distributor[0] = 12;
  distributor[1] = 24;
  put(&mut distributor, 8, &platform::DISTRIBUTOR.to_le_bytes());
  distributor[20] = 3;
  body.extend(distributor);
  let mut redistributor = [0; 16];
  redistributor[0] = 14;
  redistributor[1] = 16;
  put(
    &mut redistributor,
    4,
    &platform::REDISTRIBUTOR.to_le_bytes(),
  );
  put(
    &mut redistributor,
    12,
    &(topology.redistributor_size as u32).to_le_bytes(),
  );
  body.extend(redistributor);
  if let Some(msi) = topology.msi {
    let mut frame = [0; 24];
    frame[0] = 13;
    frame[1] = 24;
    put(&mut frame, 8, &platform::MSI.to_le_bytes());
    put(&mut frame, 16, &1u32.to_le_bytes());
    put(&mut frame, 20, &(msi.count as u16).to_le_bytes());
    put(&mut frame, 22, &(msi.first as u16).to_le_bytes());
    body.extend(frame);
  }
  table(b"APIC", 5, &body)
}

pub(super) fn timers() -> Vec<u8> {
  let mut body = vec![0; 104 - 36];
  put(&mut body, 0, &u64::MAX.to_le_bytes());
  for (offset, irq) in [(48, 29u32), (56, 30), (64, 27), (72, 26)] {
    put(&mut body, offset - 36, &irq.to_le_bytes());
    put(&mut body, offset + 4 - 36, &4u32.to_le_bytes());
  }
  put(&mut body, 80 - 36, &u64::MAX.to_le_bytes());
  table(b"GTDT", 3, &body)
}
