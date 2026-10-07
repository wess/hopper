mod support;

use machine::{
  devices::virtio::{
    gpu,
    queue::{self, Buffer, Chain},
  },
  dma::Span,
};
use support::{descriptor, Ram, BASE};

fn submit(display: &mut gpu::Display, ram: &mut Ram, command: u32, body: &[u8]) -> Vec<u8> {
  let mut data = vec![0; 24];
  data[..4].copy_from_slice(&command.to_le_bytes());
  data[4..8].copy_from_slice(&1u32.to_le_bytes());
  data[8..16].copy_from_slice(&12345u64.to_le_bytes());
  data.extend(body);
  ram.0[0x1000..0x1000 + data.len()].copy_from_slice(&data);
  descriptor(ram, 0, BASE + 0x1000, 13, 1, 1);
  descriptor(ram, 1, BASE + 0x100d, (data.len() - 13) as u32, 1, 2);
  descriptor(ram, 2, BASE + 0x2000, 11, 3, 3);
  descriptor(ram, 3, BASE + 0x200b, 397, 2, 0);
  ram.0[0x100..0x110].fill(0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  let mut queue = queue::create(ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap();
  let chain = queue::pop(&mut queue, ram).unwrap().unwrap();
  let length = gpu::execute(display, ram, &chain).unwrap();
  queue::complete(&mut queue, ram, &chain, length).unwrap();
  let response = ram.0[0x2000..0x2000 + length as usize].to_vec();
  assert_eq!(&response[4..16], &data[4..16]);
  response
}

fn words(values: &[u32]) -> Vec<u8> {
  values
    .iter()
    .flat_map(|value| value.to_le_bytes())
    .collect()
}

fn status(bytes: &[u8]) -> u32 {
  u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

fn ok(display: &mut gpu::Display, ram: &mut Ram, command: u32, body: &[u8]) {
  assert_eq!(status(&submit(display, ram, command, body)), 0x1100);
}

#[test]
fn fragmented_backing_transfer_crop_and_flush_preserve_display_until_published() {
  let mut display = gpu::create(1024, 768).unwrap();
  let mut ram = Ram::default();
  let info = submit(&mut display, &mut ram, 0x100, &[]);
  assert_eq!(status(&info), 0x1101);
  assert_eq!(&info[32..44], &words(&[1024, 768, 1]));
  assert!(info[48..].iter().all(|byte| *byte == 0));
  ok(&mut display, &mut ram, 0x101, &words(&[1, 2, 4, 3]));
  let mut backing = words(&[1, 2]);
  backing.extend((BASE + 0x3000).to_le_bytes());
  backing.extend(words(&[19, 0]));
  backing.extend((BASE + 0x4000).to_le_bytes());
  backing.extend(words(&[29, 0]));
  ok(&mut display, &mut ram, 0x106, &backing);
  let pixels: Vec<_> = (0..12u8)
    .flat_map(|value| [value, value + 20, value + 40, 0])
    .collect();
  ram.0[0x3000..0x3013].copy_from_slice(&pixels[..19]);
  ram.0[0x4000..0x401d].copy_from_slice(&pixels[19..]);
  ok(&mut display, &mut ram, 0x103, &words(&[1, 1, 2, 2, 0, 1]));
  let before = gpu::frame(&display).unwrap().generation;
  let mut transfer = words(&[1, 1, 2, 2]);
  transfer.extend(20u64.to_le_bytes());
  transfer.extend(words(&[1, 0]));
  ok(&mut display, &mut ram, 0x105, &transfer);
  assert_eq!(gpu::frame(&display).unwrap().generation, before);
  assert!(gpu::frame(&display)
    .unwrap()
    .rgba
    .as_chunks::<4>()
    .0
    .iter()
    .all(|pixel| *pixel == [0, 0, 0, 255]));
  ok(&mut display, &mut ram, 0x104, &words(&[1, 1, 2, 2, 1, 0]));
  let frame = gpu::frame(&display).unwrap();
  assert_eq!((frame.width, frame.height), (2, 2));
  assert_eq!(
    frame.rgba,
    [45, 25, 5, 255, 46, 26, 6, 255, 49, 29, 9, 255, 50, 30, 10, 255]
  );
  assert!(frame.generation > before);
  ok(&mut display, &mut ram, 0x107, &words(&[1, 0]));
  assert_eq!(
    status(&submit(&mut display, &mut ram, 0x105, &transfer)),
    0x1205
  );
  ok(&mut display, &mut ram, 0x102, &words(&[1, 0]));
  assert!(gpu::frame(&display).is_none());
}

#[test]
fn malformed_requests_do_not_create_resources_or_read_outside_guest_ram() {
  let mut display = gpu::create(640, 480).unwrap();
  let mut ram = Ram::default();
  for body in [
    words(&[0, 2, 1, 1]),
    words(&[1, 999, 1, 1]),
    words(&[1, 2, u32::MAX, 2]),
  ] {
    assert_eq!(
      status(&submit(&mut display, &mut ram, 0x101, &body)),
      0x1205
    );
  }
  ok(&mut display, &mut ram, 0x101, &words(&[1, 2, 2, 2]));
  assert_eq!(
    status(&submit(
      &mut display,
      &mut ram,
      0x101,
      &words(&[1, 2, 2, 2])
    )),
    0x1205
  );
  let mut backing = words(&[1, 1]);
  backing.extend(u64::MAX.to_le_bytes());
  backing.extend(words(&[16, 0]));
  assert_eq!(
    status(&submit(&mut display, &mut ram, 0x106, &backing)),
    0x1205
  );
  assert_eq!(
    status(&submit(&mut display, &mut ram, 0x106, &words(&[1, 4097]))),
    0x1205
  );
  assert_eq!(
    status(&submit(
      &mut display,
      &mut ram,
      0x103,
      &words(&[0, 0, 3, 2, 0, 1])
    )),
    0x1205
  );
  assert_eq!(
    status(&submit(
      &mut display,
      &mut ram,
      0x103,
      &words(&[0, 0, 2, 2, 1, 1])
    )),
    0x1202
  );
  assert_eq!(
    status(&submit(
      &mut display,
      &mut ram,
      0x104,
      &words(&[0, 0, 1, 1, 2, 0])
    )),
    0x1203
  );
  assert_eq!(status(&submit(&mut display, &mut ram, 0x200, &[])), 0x1200);
  assert_eq!(status(&submit(&mut display, &mut ram, 0x101, &[])), 0x1205);
  assert!(gpu::frame(&display).is_none());
  gpu::reset(&mut display);
  ok(&mut display, &mut ram, 0x101, &words(&[1, 2, 2, 2]));
}

#[test]
fn insufficient_reply_space_rejects_command_before_mutating_device() {
  let mut display = gpu::create(640, 480).unwrap();
  let mut ram = Ram::default();
  let data = [words(&[0x101, 0, 0, 0, 0, 0]), words(&[1, 2, 2, 2])].concat();
  ram.0[..data.len()].copy_from_slice(&data);
  let chain = Chain {
    head: 0,
    buffers: vec![
      Buffer {
        span: Span {
          address: BASE,
          length: data.len() as u32,
        },
        writable: false,
      },
      Buffer {
        span: Span {
          address: BASE + 0x2000,
          length: 23,
        },
        writable: true,
      },
    ],
  };
  assert!(gpu::execute(&mut display, &mut ram, &chain).is_err());
  ok(&mut display, &mut ram, 0x101, &words(&[1, 2, 2, 2]));
}

#[test]
fn pixel_formats_convert_to_rgba_and_transfer_offsets_cannot_wrap() {
  for (format, expected) in [
    (1, [30, 20, 10, 40]),
    (2, [30, 20, 10, 255]),
    (3, [20, 30, 40, 10]),
    (4, [20, 30, 40, 255]),
    (67, [10, 20, 30, 40]),
    (68, [40, 30, 20, 255]),
    (121, [40, 30, 20, 10]),
    (134, [10, 20, 30, 255]),
  ] {
    let mut display = gpu::create(640, 480).unwrap();
    let mut ram = Ram::default();
    ok(&mut display, &mut ram, 0x101, &words(&[1, format, 1, 1]));
    let mut backing = words(&[1, 1]);
    backing.extend((BASE + 0x3000).to_le_bytes());
    backing.extend(words(&[4, 0]));
    ok(&mut display, &mut ram, 0x106, &backing);
    ram.0[0x3000..0x3004].copy_from_slice(&[10, 20, 30, 40]);
    let mut transfer = words(&[0, 0, 1, 1]);
    transfer.extend(u64::MAX.to_le_bytes());
    transfer.extend(words(&[1, 0]));
    assert_eq!(
      status(&submit(&mut display, &mut ram, 0x105, &transfer)),
      0x1205
    );
    transfer[16..24].fill(0);
    ok(&mut display, &mut ram, 0x105, &transfer);
    ok(&mut display, &mut ram, 0x103, &words(&[0, 0, 1, 1, 0, 1]));
    assert_eq!(gpu::frame(&display).unwrap().rgba, expected);
    ok(&mut display, &mut ram, 0x103, &words(&[0, 0, 0, 0, 0, 0]));
    assert!(gpu::frame(&display).is_none());
  }
}

#[test]
fn resource_count_is_bounded_and_unref_reclaims_capacity() {
  let mut display = gpu::create(640, 480).unwrap();
  let mut ram = Ram::default();
  for id in 1..=64 {
    ok(&mut display, &mut ram, 0x101, &words(&[id, 2, 1, 1]));
  }
  assert_eq!(
    status(&submit(
      &mut display,
      &mut ram,
      0x101,
      &words(&[65, 2, 1, 1])
    )),
    0x1201
  );
  ok(&mut display, &mut ram, 0x102, &words(&[1, 0]));
  ok(&mut display, &mut ram, 0x101, &words(&[65, 2, 1, 1]));
}

#[test]
fn native_virtio_mode_replaces_the_firmware_linear_framebuffer() {
  let mut display = gpu::create(640, 480).unwrap();
  let mut ram = Ram::default();
  for (index, value) in [BASE as u32 + 0x3000, 0, 2, 2, 2, 1].into_iter().enumerate() {
    gpu::linear_write(&mut display, &ram, 8 + index as u64 * 4, value).unwrap();
  }
  gpu::refresh(&mut display, &ram).unwrap();
  assert_eq!(gpu::frame(&display).unwrap().width, 2);
  ok(&mut display, &mut ram, 0x101, &words(&[1, 2, 3, 3]));
  ok(&mut display, &mut ram, 0x103, &words(&[0, 0, 3, 3, 0, 1]));
  assert_eq!(gpu::linear_read(&display, 28), 0);
  assert_eq!(gpu::frame(&display).unwrap().width, 3);
  gpu::reset(&mut display);
  assert!(gpu::frame(&display).is_none());
}

#[test]
fn cursor_image_hotspot_motion_and_hiding_are_independent_of_scanout() {
  let mut display = gpu::create(1024, 768).unwrap();
  let mut ram = Ram::default();
  ok(&mut display, &mut ram, 0x101, &words(&[1, 1, 64, 64]));
  let mut backing = words(&[1, 1]);
  backing.extend((BASE + 0x4000).to_le_bytes());
  backing.extend(words(&[64 * 64 * 4, 0]));
  ok(&mut display, &mut ram, 0x106, &backing);
  ram.0[0x4000..0x4004].copy_from_slice(&[10, 20, 30, 40]);
  let mut transfer = words(&[0, 0, 64, 64]);
  transfer.extend(0u64.to_le_bytes());
  transfer.extend(words(&[1, 0]));
  ok(&mut display, &mut ram, 0x105, &transfer);
  let chain = Chain {
    head: 0,
    buffers: vec![Buffer {
      span: Span {
        address: BASE + 0x1000,
        length: 56,
      },
      writable: false,
    }],
  };
  let mut command = words(&[0x300, 0, 0, 0, 0, 0, 0, 300, 400, 0, 1, 2, 3, 0]);
  ram.0[0x1000..0x1038].copy_from_slice(&command);
  assert_eq!(
    gpu::execute_cursor(&mut display, &mut ram, &chain).unwrap(),
    0
  );
  let cursor = gpu::cursor(&display);
  assert_eq!(
    (cursor.x, cursor.y, cursor.hot_x, cursor.hot_y),
    (300, 400, 2, 3)
  );
  assert_eq!(&cursor.rgba.as_ref().unwrap()[..4], &[30, 20, 10, 40]);
  assert!(gpu::frame(&display).is_none());
  command[..4].copy_from_slice(&0x301u32.to_le_bytes());
  command[28..32].copy_from_slice(&500u32.to_le_bytes());
  command[40..56].fill(255);
  ram.0[0x1000..0x1038].copy_from_slice(&command);
  gpu::execute_cursor(&mut display, &mut ram, &chain).unwrap();
  assert_eq!(gpu::cursor(&display).x, 500);
  assert_eq!(gpu::cursor(&display).hot_x, 2);
  command[..4].copy_from_slice(&0x300u32.to_le_bytes());
  command[40..44].copy_from_slice(&1u32.to_le_bytes());
  ram.0[0x1000..0x1038].copy_from_slice(&command);
  assert!(gpu::execute_cursor(&mut display, &mut ram, &chain).is_err());
  assert_eq!(gpu::cursor(&display).hot_x, 2);
  command[40..44].fill(0);
  ram.0[0x1000..0x1038].copy_from_slice(&command);
  gpu::execute_cursor(&mut display, &mut ram, &chain).unwrap();
  assert!(gpu::cursor(&display).rgba.is_none());
}
