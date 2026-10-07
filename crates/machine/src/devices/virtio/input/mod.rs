mod config;
mod events;

use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
  Keyboard,
  Tablet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
  pub kind: u16,
  pub code: u16,
  pub value: i32,
}

pub const SYN: Event = Event {
  kind: 0,
  code: 0,
  value: 0,
};

pub struct Input {
  kind: Kind,
  select: u8,
  subselect: u8,
  pending: VecDeque<Event>,
  held: BTreeSet<u16>,
  leds: u8,
}

pub use config::{read as config, write as configure};
pub use events::{enqueue, execute, release};

pub fn create(kind: Kind) -> Input {
  Input {
    kind,
    select: 0,
    subselect: 0,
    pending: VecDeque::new(),
    held: BTreeSet::new(),
    leds: 0,
  }
}

pub fn pending(input: &Input) -> usize {
  input.pending.len()
}
pub fn leds(input: &Input) -> u8 {
  input.leds
}

pub fn reset(input: &mut Input) {
  input.select = 0;
  input.subselect = 0;
  input.pending.clear();
  input.held.clear();
  input.leds = 0;
}
