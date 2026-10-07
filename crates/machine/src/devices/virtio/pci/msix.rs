use anyhow::ensure;

pub const SIZE: u64 = 0x1000;
pub const PENDING: u64 = 0x800;

pub struct State {
  entries: Vec<[u32; 4]>,
  pending: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Message {
  pub address: u64,
  pub data: u32,
}

pub fn create(count: usize) -> anyhow::Result<State> {
  ensure!((1..=64).contains(&count), "Invalid MSI-X table size");
  Ok(State {
    entries: vec![[0, 0, 0, 1]; count],
    pending: 0,
  })
}

pub fn count(state: &State) -> usize {
  state.entries.len()
}

fn access(offset: u64, width: usize) -> anyhow::Result<()> {
  ensure!(
    matches!(width, 1 | 2 | 4 | 8)
      && offset.is_multiple_of(width as u64)
      && offset
        .checked_add(width as u64)
        .is_some_and(|end| end <= SIZE),
    "Invalid MSI-X table access"
  );
  Ok(())
}

pub fn read(state: &State, offset: u64, width: usize) -> anyhow::Result<u64> {
  access(offset, width)?;
  let mut value = 0;
  for index in 0..width {
    let address = offset as usize + index;
    let byte = if address < state.entries.len() * 16 {
      state.entries[address / 16][(address % 16) / 4].to_le_bytes()[address % 4]
    } else if (PENDING as usize..PENDING as usize + 8).contains(&address) {
      state.pending.to_le_bytes()[address - PENDING as usize]
    } else {
      0
    };
    value |= (byte as u64) << (index * 8);
  }
  Ok(value)
}

pub fn write(state: &mut State, offset: u64, width: usize, value: u64) -> anyhow::Result<()> {
  access(offset, width)?;
  for (index, byte) in value.to_le_bytes().iter().take(width).enumerate() {
    let address = offset as usize + index;
    if address < state.entries.len() * 16 {
      let entry = &mut state.entries[address / 16];
      let word = &mut entry[(address % 16) / 4];
      let shift = (address % 4) * 8;
      *word = (*word & !(0xff << shift)) | (*byte as u32) << shift;
      entry[3] &= 1;
    }
  }
  Ok(())
}

pub fn raise(state: &mut State, vector: u16) {
  if (vector as usize) < state.entries.len() {
    state.pending |= 1u64 << vector;
  }
}

pub fn clear(state: &mut State) {
  state.pending = 0;
}

pub fn take(state: &mut State, enabled: bool, masked: bool) -> Vec<Message> {
  if !enabled || masked {
    return Vec::new();
  }
  let mut messages = Vec::new();
  for (index, [low, high, data, control]) in state.entries.iter().enumerate() {
    let bit = 1u64 << index;
    if state.pending & bit != 0 && control & 1 == 0 {
      messages.push(Message {
        address: *low as u64 | (*high as u64) << 32,
        data: *data,
      });
      state.pending &= !bit;
    }
  }
  messages
}
