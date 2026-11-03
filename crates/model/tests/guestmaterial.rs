use model::{
  guest::wire::auth::{Boot, GuestHandshake, HostHandshake, Material, Scope, Secret},
  GuestOs,
};

fn encoded() -> Vec<u8> {
  let material = Material {
    boot: Boot {
      machine: "f196c47e-bd9e-4e84-a49c-354b95091819".into(),
      generation: 9,
      guest: GuestOs::Linux,
    },
    key: Secret::from_bytes([11; 32]).unwrap(),
  };
  let mut bytes = Vec::new();
  material.write(&mut bytes).unwrap();
  bytes
}

#[test]
fn boot_material_is_fixed_size_and_authenticates_the_expected_boot() {
  let bytes = encoded();
  assert_eq!(bytes.len(), Material::LENGTH);
  let material = Material::read(&mut bytes.as_slice()).unwrap();
  assert_eq!(material.boot.generation, 9);
  let boot = material.boot.clone();
  let host = HostHandshake::begin(
    material.key,
    Scope {
      boot: boot.clone(),
      policy: 17,
    },
  )
  .unwrap();
  let material = Material::read(&mut bytes.as_slice()).unwrap();
  let (guest, answer) = GuestHandshake::answer(material.key, &boot, &host.hello()).unwrap();
  let (_, confirmation) = host.finish(&answer).unwrap();
  assert_eq!(guest.finish(&confirmation).unwrap().scope().policy, 17);
}

#[test]
fn truncated_extended_zero_or_unknown_boot_material_rejects() {
  let bytes = encoded();
  for length in 0..bytes.len() {
    assert!(Material::read(&mut &bytes[..length]).is_err());
  }
  let mut extended = bytes.clone();
  extended.push(0);
  assert!(Material::read(&mut extended.as_slice()).is_err());
  for (start, end) in [(0, 4), (4, 20), (20, 28), (28, 29), (29, 61)] {
    let mut bad = bytes.clone();
    bad[start..end].fill(0);
    assert!(Material::read(&mut bad.as_slice()).is_err());
  }
  let mut bad = bytes;
  bad[28] = 255;
  assert!(Material::read(&mut bad.as_slice()).is_err());
}

#[test]
fn invalid_boot_identity_cannot_be_written() {
  let material = Material {
    boot: Boot {
      machine: "../another-vm".into(),
      generation: 1,
      guest: GuestOs::Linux,
    },
    key: Secret::from_bytes([11; 32]).unwrap(),
  };
  let mut bytes = Vec::new();
  assert!(material.write(&mut bytes).is_err());
  assert!(bytes.is_empty());
}

#[test]
fn retained_material_creates_fresh_authenticated_sessions_with_original_scope() {
  let bytes = encoded();
  let material = Material::read(&mut bytes.as_slice()).unwrap();
  let first = material.handshake(17).unwrap();
  let second = material.handshake(18).unwrap();
  assert_ne!(first.hello()[37..69], second.hello()[37..69]);
  for (host, policy) in [(first, 17), (second, 18)] {
    let guest_material = Material::read(&mut bytes.as_slice()).unwrap();
    let (guest, answer) =
      GuestHandshake::answer(guest_material.key, &material.boot, &host.hello()).unwrap();
    let (session, confirmation) = host.finish(&answer).unwrap();
    assert_eq!(session.scope().boot, material.boot);
    assert_eq!(guest.finish(&confirmation).unwrap().scope().policy, policy);
  }
  let mut preserved = Vec::new();
  material.write(&mut preserved).unwrap();
  assert_eq!(preserved, bytes);
}
