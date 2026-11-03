mod support;
use machine::devices::virtio::{
  gpu::{self, snapshot},
  pci, queue,
};
use support::{Ram, BASE};

#[path = "gpu/codec.rs"]
mod codec;

fn reopen(saved: snapshot::State) -> snapshot::State {
  let mut bytes = Vec::new();
  snapshot::wire::encode(&saved, &mut bytes, || Ok(())).unwrap();
  drop(saved);
  snapshot::wire::decode(&mut bytes.as_slice(), (640, 480), || Ok(())).unwrap()
}

fn words(values: &[u32]) -> Vec<u8> {
  values
    .iter()
    .flat_map(|value| value.to_le_bytes())
    .collect()
}

fn command(display: &mut gpu::Display, ram: &mut Ram, kind: u32, body: &[u8]) {
  let mut data = vec![0; 24];
  data[..4].copy_from_slice(&kind.to_le_bytes());
  data.extend_from_slice(body);
  ram.0[0x1000..0x1000 + data.len()].copy_from_slice(&data);
  support::descriptor(ram, 0, BASE + 0x1000, data.len() as u32, 1, 1);
  support::descriptor(ram, 1, BASE + 0x2000, 408, 2, 0);
  ram.0[0x100..0x110].fill(0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  let mut queue = queue::create(ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap();
  let chain = queue::pop(&mut queue, ram).unwrap().unwrap();
  let length = if kind >= 0x300 {
    gpu::execute_cursor(display, ram, &chain).unwrap()
  } else {
    gpu::execute(display, ram, &chain).unwrap()
  };
  queue::complete(&mut queue, ram, &chain, length).unwrap();
  assert_eq!(&ram.0[0x2000..0x2004], &0x1100u32.to_le_bytes());
}

fn transfer(display: &mut gpu::Display, ram: &mut Ram, id: u32, width: u32, height: u32) {
  let mut body = words(&[0, 0, width, height]);
  body.extend(0u64.to_le_bytes());
  body.extend(words(&[id, 0]));
  command(display, ram, 0x105, &body);
}

fn display(ram: &mut Ram) -> gpu::Display {
  let mut display = gpu::create(640, 480).unwrap();
  for (id, width, height, offset, length) in [(1, 2, 2, 0x3000, 16), (2, 64, 64, 0x4000, 16384)] {
    command(&mut display, ram, 0x101, &words(&[id, 1, width, height]));
    let mut backing = words(&[id, 1]);
    backing.extend((BASE + offset).to_le_bytes());
    backing.extend(words(&[length, 0]));
    command(&mut display, ram, 0x106, &backing);
    for pixel in ram.0[offset as usize..offset as usize + length as usize].chunks_mut(4) {
      pixel.copy_from_slice(&[1, 2, 3, 4]);
    }
    transfer(&mut display, ram, id, width, height);
  }
  command(&mut display, ram, 0x103, &words(&[0, 0, 2, 2, 0, 1]));
  command(
    &mut display,
    ram,
    0x300,
    &words(&[0, 100, 101, 0, 2, 3, 4, 0]),
  );
  display
}

#[test]
fn capture_preserves_published_frame_unpublished_pixels_cursor_and_backing() {
  let mut ram = Ram::default();
  let mut original = display(&mut ram);
  ram.0[0x3000..0x3010].fill(10);
  transfer(&mut original, &mut ram, 1, 2, 2);
  let saved = reopen(snapshot::capture(&original));
  let captured_ram = ram.0.clone();
  ram.0[0x3000..0x3010].fill(20);
  transfer(&mut original, &mut ram, 1, 2, 2);
  command(&mut original, &mut ram, 0x104, &words(&[0, 0, 2, 2, 1, 0]));
  assert_eq!(gpu::frame(&original).unwrap().rgba, vec![20; 16]);
  gpu::reset(&mut original);
  drop(original);
  for _ in 0..2 {
    let mut ram = Ram(captured_ram.clone());
    let mut restored = snapshot::restore(&saved, &gpu::create(640, 480).unwrap(), &ram).unwrap();
    assert_eq!(gpu::frame(&restored).unwrap().rgba, [3, 2, 1, 4].repeat(4));
    let cursor = gpu::cursor(&restored);
    assert_eq!(
      (cursor.x, cursor.y, cursor.hot_x, cursor.hot_y),
      (100, 101, 3, 4)
    );
    assert_eq!(cursor.rgba.as_ref().unwrap(), &[3, 2, 1, 4].repeat(4096));
    command(&mut restored, &mut ram, 0x104, &words(&[0, 0, 2, 2, 1, 0]));
    assert_eq!(gpu::frame(&restored).unwrap().rgba, vec![10; 16]);
    ram.0[0x3000..0x3010].fill(30);
    transfer(&mut restored, &mut ram, 1, 2, 2);
    command(&mut restored, &mut ram, 0x104, &words(&[0, 0, 2, 2, 1, 0]));
    assert_eq!(gpu::frame(&restored).unwrap().rgba, vec![30; 16]);
  }
}

#[test]
fn complete_pci_graphics_restore_validates_backing_before_destination_mutation() {
  let mut ram = Ram::default();
  let original = pci::graphics(display(&mut ram)).unwrap();
  let saved = pci::snapshot::capture(&original).unwrap();
  let transport = pci::snapshot::wire::encode(&saved).unwrap();
  let mut bytes = Vec::new();
  pci::snapshot::gpuwire::encode(&saved, &mut bytes, || Ok(())).unwrap();
  drop(saved);
  let expected = pci::graphics(gpu::create(640, 480).unwrap()).unwrap();
  let saved = pci::snapshot::records::decode(
    &transport,
    pci::snapshot::records::Contents::Graphics(&mut bytes.as_slice()),
    &expected,
    || Ok(()),
  )
  .unwrap();
  drop(original);
  let mut target = pci::graphics(gpu::create(640, 480).unwrap()).unwrap();
  assert!(pci::snapshot::restore(&saved, &mut target, &Ram(vec![0; 0x3008])).is_err());
  assert!(gpu::frame(pci::display(&target).unwrap()).is_none());
  assert_eq!(pci::read(&mut target, 20, 1).unwrap(), 0);
  pci::snapshot::restore(&saved, &mut target, &ram).unwrap();
  assert_eq!(
    gpu::frame(pci::display(&target).unwrap()).unwrap().rgba,
    [3, 2, 1, 4].repeat(4)
  );
  let mut foreign = pci::graphics(gpu::create(800, 600).unwrap()).unwrap();
  assert!(pci::snapshot::restore(&saved, &mut foreign, &ram).is_err());
  assert!(gpu::frame(pci::display(&foreign).unwrap()).is_none());
}

#[test]
fn linear_capture_preserves_active_layout_separately_from_pending_registers() {
  let mut ram = Ram::default();
  let mut original = gpu::create(640, 480).unwrap();
  ram.0[0x3000..0x3008].copy_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
  for (offset, value) in [
    (8, (BASE + 0x3000) as u32),
    (12, 0),
    (16, 2),
    (20, 1),
    (24, 3),
    (28, 1),
  ] {
    gpu::linear_write(&mut original, &ram, offset, value).unwrap();
  }
  gpu::refresh(&mut original, &ram).unwrap();
  gpu::linear_write(&mut original, &ram, 16, u32::MAX).unwrap();
  let saved = reopen(snapshot::capture(&original));
  let captured_ram = Ram(ram.0.clone());
  ram.0[0x3000..0x3008].fill(20);
  gpu::refresh(&mut original, &ram).unwrap();
  drop(original);
  let mut restored =
    snapshot::restore(&saved, &gpu::create(640, 480).unwrap(), &captured_ram).unwrap();
  assert_eq!(gpu::linear_read(&restored, 16), u32::MAX);
  assert_eq!(
    gpu::frame(&restored).unwrap().rgba,
    [3, 2, 1, 255, 6, 5, 4, 255]
  );
  gpu::refresh(&mut restored, &captured_ram).unwrap();
  assert_eq!(
    gpu::frame(&restored).unwrap().rgba,
    [3, 2, 1, 255, 6, 5, 4, 255]
  );
  assert!(gpu::linear_write(&mut restored, &captured_ram, 28, 1).is_err());
  assert!(snapshot::restore(
    &saved,
    &gpu::create(640, 480).unwrap(),
    &Ram(vec![0; 0x3008])
  )
  .is_err());
}
