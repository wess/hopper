#[allow(dead_code)]
#[path = "../src/vz/kernel.rs"]
mod kernel;

#[test]
fn direct_boot_accepts_arm64_image_and_rejects_compressed_efi_and_other_architectures() {
  let mut image = [0u8; 64];
  image[56..60].copy_from_slice(b"ARM\x64");
  assert!(kernel::header(&image).is_ok());
  image[0..2].copy_from_slice(b"MZ");
  assert!(kernel::header(&image).is_ok());
  image[4..8].copy_from_slice(b"zimg");
  image[56..60].fill(0);
  assert!(kernel::header(&image)
    .unwrap_err()
    .to_string()
    .contains("uncompressed"));
  assert!(kernel::header(&[0; 64]).is_err());
  assert!(kernel::header(&image[..63]).is_err());
}

#[test]
fn big_endian_kernels_are_rejected() {
  let mut image = [0u8; 64];
  image[56..60].copy_from_slice(b"ARM\x64");
  image[24] = 1;
  assert!(kernel::header(&image).is_err());
}
