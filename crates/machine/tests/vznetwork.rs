#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::vz::network::address;

#[test]
fn persistent_identity_has_a_stable_local_unicast_address() {
  let identity = b"owned independent vm identity";
  let first = address(identity).unwrap();
  assert_eq!(first, "be:b7:a9:8b:75:5c");
  assert_eq!(address(identity).unwrap(), first);
  assert_ne!(address(b"another owned vm").unwrap(), first);
  let bytes: Vec<_> = first
    .split(':')
    .map(|byte| u8::from_str_radix(byte, 16).unwrap())
    .collect();
  assert_eq!(bytes.len(), 6);
  assert_eq!(bytes[0] & 3, 2);
  assert!(address(&[]).is_err());
  assert!(address(&[0; 4097]).is_err());
  assert!(address(&[0; 4096]).is_ok());
}
