#[allow(dead_code)]
mod support;

use machine::devices::virtio::{gpu, pci};
use support::{Ram, BASE};

fn layout(device: &mut pci::Device, ram: &mut Ram, values: [u32; 6]) {
  for (index, value) in values.into_iter().enumerate() {
    pci::write(
      device,
      ram,
      gpu::linear::BASE + 8 + index as u64 * 4,
      4,
      value,
    )
    .unwrap();
  }
}

#[test]
fn direct_guest_writes_remain_visible_after_virtio_reset() {
  let mut ram = Ram::default();
  let mut device = pci::graphics(gpu::create(1024, 768).unwrap()).unwrap();
  assert_eq!(
    pci::read(&mut device, gpu::linear::BASE, 4).unwrap(),
    gpu::linear::MAGIC
  );
  layout(&mut device, &mut ram, [BASE as u32 + 0x3000, 0, 2, 2, 3, 1]);
  assert!(pci::fault(&device).is_none());
  ram.0[0x3000..0x3018].copy_from_slice(&[
    1, 2, 3, 4, 5, 6, 7, 8, 99, 99, 99, 99, 9, 10, 11, 12, 13, 14, 15, 16, 99, 99, 99, 99,
  ]);
  pci::refresh(&mut device, &ram).unwrap();
  let frame = gpu::frame(pci::display(&device).unwrap()).unwrap();
  assert_eq!(
    frame.rgba,
    [3, 2, 1, 255, 7, 6, 5, 255, 11, 10, 9, 255, 15, 14, 13, 255]
  );
  let generation = frame.generation;
  pci::refresh(&mut device, &ram).unwrap();
  assert_eq!(
    gpu::frame(pci::display(&device).unwrap())
      .unwrap()
      .generation,
    generation
  );
  pci::write(&mut device, &mut ram, 20, 1, 0).unwrap();
  ram.0[0x3000..0x3004].copy_from_slice(&[20, 30, 40, 0]);
  pci::refresh(&mut device, &ram).unwrap();
  let frame = gpu::frame(pci::display(&device).unwrap()).unwrap();
  assert_eq!(&frame.rgba[..4], &[40, 30, 20, 255]);
  assert!(frame.generation > generation);
  pci::write(&mut device, &mut ram, gpu::linear::BASE + 28, 4, 0).unwrap();
  assert!(gpu::frame(pci::display(&device).unwrap()).is_none());
}

#[test]
fn invalid_layouts_fault_without_replacing_the_active_framebuffer() {
  let mut ram = Ram::default();
  for values in [
    [u32::MAX, u32::MAX, 2, 2, 2, 1],
    [BASE as u32, 0, 0, 2, 2, 1],
    [BASE as u32, 0, 2, 2, 1, 1],
    [BASE as u32, 0, 8192, 8192, 8192, 1],
    [BASE as u32, 0, 2, 2, 2, 2],
  ] {
    let mut device = pci::graphics(gpu::create(1024, 768).unwrap()).unwrap();
    layout(&mut device, &mut ram, [BASE as u32, 0, 2, 2, 2, 1]);
    pci::refresh(&mut device, &ram).unwrap();
    let before = gpu::frame(pci::display(&device).unwrap())
      .unwrap()
      .rgba
      .clone();
    layout(&mut device, &mut ram, values);
    assert!(pci::fault(&device).is_some());
    pci::refresh(&mut device, &ram).unwrap();
    assert_eq!(
      gpu::frame(pci::display(&device).unwrap()).unwrap().rgba,
      before
    );
  }
}
