use crate::dma::{Memory, Span};
use anyhow::{ensure, Context};
use std::{
  collections::BTreeSet,
  sync::atomic::{fence, Ordering},
};

pub struct Buffer {
  pub span: Span,
  pub writable: bool,
}

pub struct Chain {
  pub head: u16,
  pub buffers: Vec<Buffer>,
}

pub struct Queue {
  size: u16,
  descriptor: u64,
  available: u64,
  used: u64,
  next_available: u16,
  next_used: u16,
  pending: BTreeSet<u16>,
}

pub fn create(
  memory: &mut impl Memory,
  size: u16,
  descriptor: u64,
  available: u64,
  used: u64,
) -> anyhow::Result<Queue> {
  ensure!(
    size > 0 && size <= 32768 && size.is_power_of_two(),
    "Invalid Virtio queue size"
  );
  let regions = [
    (descriptor, 16 * size as usize, 16),
    (available, 6 + 2 * size as usize, 2),
    (used, 6 + 8 * size as usize, 4),
  ];
  for (index, (address, length, alignment)) in regions.iter().enumerate() {
    ensure!(
      address.is_multiple_of(*alignment) && memory.contains(*address, *length),
      "Virtio queue region is unaligned or outside guest RAM"
    );
    let end = address
      .checked_add(*length as u64)
      .context("Virtio queue address overflow")?;
    for (other, length, _) in &regions[..index] {
      let other_end = other
        .checked_add(*length as u64)
        .context("Virtio queue address overflow")?;
      ensure!(
        end <= *other || other_end <= *address,
        "Virtio queue regions overlap"
      );
    }
  }
  memory.write(used, &[0; 4])?;
  Ok(Queue {
    size,
    descriptor,
    available,
    used,
    next_available: 0,
    next_used: 0,
    pending: BTreeSet::new(),
  })
}

fn word(memory: &impl Memory, address: u64) -> anyhow::Result<u16> {
  let mut bytes = [0; 2];
  memory.read(address, &mut bytes)?;
  Ok(u16::from_le_bytes(bytes))
}

pub fn pop(queue: &mut Queue, memory: &impl Memory) -> anyhow::Result<Option<Chain>> {
  let available = word(memory, queue.available + 2)?;
  fence(Ordering::Acquire);
  let count = available.wrapping_sub(queue.next_available);
  ensure!(
    count as usize + queue.pending.len() <= queue.size as usize,
    "Virtio driver overran its available ring"
  );
  if count == 0 {
    return Ok(None);
  }
  let slot = queue.next_available % queue.size;
  let head = word(memory, queue.available + 4 + 2 * slot as u64)?;
  ensure!(
    !queue.pending.contains(&head),
    "Virtio descriptor head is already pending"
  );
  let mut index = head;
  let mut visited = vec![false; queue.size as usize];
  let mut buffers = Vec::new();
  let mut writable_seen = false;
  let mut length = 0u64;
  loop {
    ensure!(
      index < queue.size && !visited[index as usize],
      "Virtio descriptor chain is invalid or cyclic"
    );
    visited[index as usize] = true;
    let mut bytes = [0; 16];
    memory.read(queue.descriptor + 16 * index as u64, &mut bytes)?;
    let address = u64::from_le_bytes(bytes[..8].try_into()?);
    let size = u32::from_le_bytes(bytes[8..12].try_into()?);
    let flags = u16::from_le_bytes(bytes[12..14].try_into()?);
    ensure!(
      flags & 4 == 0,
      "Indirect Virtio descriptors were not negotiated"
    );
    ensure!(
      memory.contains(address, size as usize),
      "Virtio buffer exceeds guest RAM"
    );
    length += size as u64;
    ensure!(length <= 1 << 32, "Virtio descriptor chain exceeds 4 GiB");
    let writable = flags & 2 != 0;
    ensure!(
      !writable_seen || writable,
      "Virtio readable buffers follow writable buffers"
    );
    writable_seen |= writable;
    buffers.push(Buffer {
      span: Span {
        address,
        length: size,
      },
      writable,
    });
    if flags & 1 == 0 {
      break;
    }
    index = u16::from_le_bytes(bytes[14..16].try_into()?);
  }
  queue.pending.insert(head);
  queue.next_available = queue.next_available.wrapping_add(1);
  Ok(Some(Chain { head, buffers }))
}

pub fn complete(
  queue: &mut Queue,
  memory: &mut impl Memory,
  chain: &Chain,
  length: u32,
) -> anyhow::Result<bool> {
  ensure!(
    queue.pending.contains(&chain.head),
    "Virtio request is not pending"
  );
  let capacity: u64 = chain
    .buffers
    .iter()
    .filter(|buffer| buffer.writable)
    .map(|buffer| buffer.span.length as u64)
    .sum();
  ensure!(
    length as u64 <= capacity,
    "Virtio used length exceeds writable buffers"
  );
  let slot = queue.next_used % queue.size;
  let mut bytes = [0; 8];
  bytes[..4].copy_from_slice(&(chain.head as u32).to_le_bytes());
  bytes[4..].copy_from_slice(&length.to_le_bytes());
  memory.write(queue.used + 4 + 8 * slot as u64, &bytes)?;
  fence(Ordering::Release);
  let next = queue.next_used.wrapping_add(1);
  memory.write(queue.used + 2, &next.to_le_bytes())?;
  queue.next_used = next;
  queue.pending.remove(&chain.head);
  fence(Ordering::SeqCst);
  Ok(word(memory, queue.available)? & 1 == 0)
}
