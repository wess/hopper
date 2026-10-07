#[path = "../src/views/machines/native/frame.rs"]
mod frame;

fn input(width: u32, height: u32, rgba: Vec<u8>) -> host::MachineFrame {
  host::MachineFrame {
    width,
    height,
    generation: 7,
    rgba,
  }
}

#[test]
fn guest_pixels_are_uploaded_in_bgra_order_without_changing_alpha() {
  let image = frame::render(input(2, 1, vec![255, 0, 0, 255, 0, 40, 255, 120])).unwrap();
  assert_eq!(
    image.as_bytes(0).unwrap(),
    [0, 0, 255, 255, 255, 40, 0, 120]
  );
  assert_eq!(image.size(0).width.0, 2);
  assert_eq!(image.size(0).height.0, 1);
}

#[test]
fn malformed_guest_frames_are_rejected_before_image_allocation() {
  for frame in [
    input(0, 1, vec![]),
    input(4097, 1, vec![]),
    input(1, 1, vec![0; 3]),
    input(1, 1, vec![0; 5]),
  ] {
    assert!(frame::render(frame).is_err());
  }
}
