use super::{aml::package, put, table};
use crate::{
  devices::pci::{ECAM_SIZE, FIRST_IRQ},
  platform,
};

pub(super) fn mcfg() -> Vec<u8> {
  let mut body = vec![0; 24];
  put(&mut body, 8, &platform::ECAM.to_le_bytes());
  table(b"MCFG", 1, &body)
}

fn resource(kind: u8, flags: u8, width: usize, minimum: u32, length: u32) -> Vec<u8> {
  let mut bytes = vec![if width == 2 { 0x88 } else { 0x87 }];
  bytes.extend((3u16 + 5 * width as u16).to_le_bytes());
  bytes.extend([kind, 0x0c, flags]);
  for value in [0, minimum, minimum + length - 1, 0, length] {
    bytes.extend(&value.to_le_bytes()[..width]);
  }
  bytes
}

fn current(mut resources: Vec<u8>) -> Vec<u8> {
  resources.extend([0x79, 0]);
  let mut buffer = vec![0x0b];
  buffer.extend((resources.len() as u16).to_le_bytes());
  buffer.extend(resources);
  let mut bytes = b"\x08_CRS".to_vec();
  bytes.extend(package(&[0x11], &buffer));
  bytes
}

pub(super) fn namespace() -> Vec<u8> {
  let mut root = b"PCI0\x08_HID\x0dPNP0A08\0\x08_CID\x0dPNP0A03\0".to_vec();
  root.extend(b"\x08_UID\0\x08_SEG\0\x08_BBN\0\x08_CCA\x01\x08_STA\x0a\x0f");
  let mut resources = resource(2, 0, 2, 0, 1);
  resources.extend(resource(
    0,
    1,
    4,
    platform::PCI_MEMORY as u32,
    platform::PCI_MEMORY_SIZE as u32,
  ));
  root.extend(current(resources));
  let mut routes = vec![128];
  for device in 0..32u32 {
    for pin in 0..4u32 {
      let mut route = vec![4, 0x0c];
      route.extend(((device << 16) | 0xffff).to_le_bytes());
      route.extend([
        0x0a,
        pin as u8,
        0,
        0x0a,
        (FIRST_IRQ + (device + pin) % 4) as u8,
      ]);
      routes.extend(package(&[0x12], &route));
    }
  }
  root.extend(b"\x08_PRT");
  root.extend(package(&[0x12], &routes));
  let mut bytes = package(&[0x5b, 0x82], &root);
  let mut reserved = b"RES0\x08_HID\x0dPNP0C02\0\x08_UID\0".to_vec();
  let mut ecam = vec![0x86, 9, 0, 1];
  ecam.extend((platform::ECAM as u32).to_le_bytes());
  ecam.extend((ECAM_SIZE as u32).to_le_bytes());
  reserved.extend(current(ecam));
  bytes.extend(package(&[0x5b, 0x82], &reserved));
  bytes
}
