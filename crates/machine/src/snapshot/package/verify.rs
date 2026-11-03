use super::Package;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::io::Read;
use zeroize::Zeroizing;

impl Package {
  /// Verify every component body against its manifest-bound footer before disrupting a source VM.
  /// This is an integrity precheck; component schemas and separately stored images still need decoding.
  pub fn verify_components(
    &self,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    authorize()?;
    let mut bytes = Zeroizing::new([0; 65536]);
    for record in self.layout.records()? {
      let mut reader = self.reader(record, &mut authorize)?;
      let mut digest = Sha256::new();
      let mut tail = Zeroizing::new(Vec::with_capacity(65568));
      loop {
        let count = reader.read(&mut bytes[..])?;
        if count == 0 {
          break;
        }
        tail.extend_from_slice(&bytes[..count]);
        if tail.len() > 32 {
          let prefix = tail.len() - 32;
          digest.update(&tail[..prefix]);
          tail.copy_within(prefix.., 0);
          tail.truncate(32);
        }
      }
      reader.finish()?;
      ensure!(
        tail.len() == 32 && digest.finalize()[..] == tail[..],
        "Snapshot component body checksum mismatch: {}",
        record.name()
      );
      authorize()?;
    }
    authorize()
  }
}
