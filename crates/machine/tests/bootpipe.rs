#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::vz::{bootstrap, queue::Check};
use model::{
  guest::wire::auth::{Boot, Material, Secret},
  GuestOs,
};
use std::{
  io::Read,
  os::fd::AsRawFd,
  sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
  },
};

fn material(guest: GuestOs) -> Material {
  Material {
    boot: Boot {
      machine: "f2bc62e7-a5e5-445c-a29e-643ed28b4211".parse().unwrap(),
      generation: 1,
      guest,
    },
    key: Secret::from_bytes([7; 32]).unwrap(),
  }
}

#[test]
fn private_pipe_has_exact_material_eof_and_close_on_exec_flags() {
  let material = material(GuestOs::Linux);
  let check: Check = Arc::new(|| Ok(()));
  let mut input = bootstrap::input(&material, &check).unwrap();
  assert_ne!(
    unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
    0
  );
  assert!(!input.poll(std::time::Instant::now()).unwrap());
  assert_eq!(
    unsafe { libc::write(input.as_raw_fd(), Material::REQUEST.as_ptr().cast(), 4) },
    4
  );
  assert!(input.poll(std::time::Instant::now()).unwrap());
  let received = Material::read(&mut input).unwrap();
  assert_eq!(received.boot, material.boot);
  let mut expected = Vec::new();
  let mut actual = Vec::new();
  material.write(&mut expected).unwrap();
  received.write(&mut actual).unwrap();
  assert_eq!(actual, expected);
  assert_eq!(actual.len(), Material::LENGTH);
  assert_eq!(input.read(&mut [0]).unwrap(), 0);
}

#[test]
fn authorization_is_rechecked_and_other_guest_material_rejects() {
  let calls = Arc::new(AtomicUsize::new(0));
  let observed = calls.clone();
  let check: Check = Arc::new(move || {
    anyhow::ensure!(observed.fetch_add(1, Ordering::SeqCst) == 0, "revoked");
    Ok(())
  });
  assert!(bootstrap::input(&material(GuestOs::Linux), &check).is_err());
  assert_eq!(calls.load(Ordering::SeqCst), 2);
  let check: Check = Arc::new(|| Ok(()));
  assert!(bootstrap::input(&material(GuestOs::Windows), &check).is_err());
}

#[test]
fn delayed_partial_requests_receive_nothing_until_complete() {
  let check: Check = Arc::new(|| Ok(()));
  let mut input = bootstrap::input(&material(GuestOs::Linux), &check).unwrap();
  let mut byte = [0u8];
  assert!(!input.poll(std::time::Instant::now()).unwrap());
  assert_eq!(
    unsafe {
      libc::recv(
        input.as_raw_fd(),
        byte.as_mut_ptr().cast(),
        1,
        libc::MSG_DONTWAIT,
      )
    },
    -1
  );
  assert_eq!(
    std::io::Error::last_os_error().kind(),
    std::io::ErrorKind::WouldBlock
  );
  assert_eq!(
    unsafe { libc::write(input.as_raw_fd(), Material::REQUEST.as_ptr().cast(), 3) },
    3
  );
  assert!(!input.poll(std::time::Instant::now()).unwrap());
  assert_eq!(
    unsafe {
      libc::recv(
        input.as_raw_fd(),
        byte.as_mut_ptr().cast(),
        1,
        libc::MSG_DONTWAIT,
      )
    },
    -1
  );
  assert_eq!(
    unsafe { libc::write(input.as_raw_fd(), Material::REQUEST[3..].as_ptr().cast(), 1) },
    1
  );
  assert!(input.poll(std::time::Instant::now()).unwrap());
  assert_eq!(
    Material::read(&mut input).unwrap().boot,
    material(GuestOs::Linux).boot
  );
}

#[test]
fn malformed_request_and_revocation_close_without_delivering_material() {
  use std::sync::atomic::AtomicBool;
  let revoked = Arc::new(AtomicBool::new(false));
  let observed = revoked.clone();
  let check: Check = Arc::new(move || {
    anyhow::ensure!(!observed.load(Ordering::SeqCst), "revoked");
    Ok(())
  });
  for invalid in [true, false] {
    let mut input = bootstrap::input(&material(GuestOs::Linux), &check).unwrap();
    let request = if invalid { b"BAD!" } else { Material::REQUEST };
    assert_eq!(
      unsafe { libc::write(input.as_raw_fd(), request.as_ptr().cast(), 4) },
      4
    );
    if !invalid {
      revoked.store(true, Ordering::SeqCst);
    }
    assert!(input.poll(std::time::Instant::now()).is_err());
    let result = input.read(&mut [0]);
    assert!(
      matches!(result, Ok(0))
        || result.is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionReset)
    );
  }
}

#[test]
fn pending_boot_delivery_expires_without_releasing_credentials() {
  let check: Check = Arc::new(|| Ok(()));
  let mut input = bootstrap::input(&material(GuestOs::Linux), &check).unwrap();
  let now = std::time::Instant::now();
  assert!(!input.poll(now).unwrap());
  assert!(input
    .poll(now + std::time::Duration::from_secs(180))
    .is_err());
  assert_eq!(input.read(&mut [0]).unwrap(), 0);
}
