use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn standard_sha256_vectors_match_with_target_acceleration() {
  for (input, expected) in [
    (
      &b""[..],
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
      &b"abc"[..],
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    ),
  ] {
    assert_eq!(hex(&Sha256::digest(input)), expected);
  }
  for chunk in [1, 7, 64, 65536] {
    let mut digest = Sha256::new();
    let bytes = vec![b'a'; chunk];
    for _ in 0..1_000_000 / chunk {
      digest.update(&bytes);
    }
    digest.update(&bytes[..1_000_000 % chunk]);
    assert_eq!(
      hex(&digest.finalize()),
      "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
  }
}
