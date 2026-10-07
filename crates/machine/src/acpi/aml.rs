pub(super) fn package(opcode: &[u8], body: &[u8]) -> Vec<u8> {
  let count = if body.len() < 63 {
    1
  } else if body.len() < 4094 {
    2
  } else {
    3
  };
  let length = body.len() + count;
  let mut bytes = opcode.to_vec();
  if count == 1 {
    bytes.push(length as u8);
  } else {
    bytes.push(((count - 1) << 6) as u8 | (length & 15) as u8);
    for index in 0..count - 1 {
      bytes.push((length >> (4 + index * 8)) as u8);
    }
  }
  bytes.extend(body);
  bytes
}

pub(super) fn namespace(topology: &crate::platform::Topology) -> Vec<u8> {
  let mut scope = b"\\_SB_".to_vec();
  for index in 0..topology.cpus {
    let mut device = format!("C{index:03X}").into_bytes();
    device.extend(b"\x08_HID\x0dACPI0007\0");
    device.extend(b"\x08_UID\x0c");
    device.extend(index.to_le_bytes());
    device.extend(b"\x08_STA\x0a\x0f");
    scope.extend(package(&[0x5b, 0x82], &device));
  }
  scope.extend(super::pci::namespace(topology.msi.as_ref()));
  package(&[0x10], &scope)
}
