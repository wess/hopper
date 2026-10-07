#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::linux::{boot, iso, kernel};
use flate2::{write::GzEncoder, Compression};
use machine::vz::queue::Check;
use std::{
  io::{Cursor, Write},
  sync::Arc,
};

#[path = "support/credentials.rs"]
mod backend;

#[path = "support/linux.rs"]
mod fixture;
use fixture::image;

fn check() -> Check {
  Arc::new(|| Ok(()))
}

#[test]
fn private_efi_staging_changes_only_owned_grub_configuration() {
  let root = tempfile::tempdir().unwrap();
  let media = fixture::media(root.path());
  let original = std::fs::read(&media).unwrap();
  let mut file = std::fs::File::open(&media).unwrap();
  assert!(iso::read(&mut file, "casper/vmlinuz", 4096, &check()).unwrap() == image());
  assert!(iso::read(&mut file, "casper/vmlinuz", 4095, &check()).is_err());
  assert!(iso::read(&mut file, "../source", 4096, &check()).is_err());
  let staged = boot::stage(&media, root.path(), &check()).unwrap();
  assert!(std::fs::read(&media).unwrap() == original);
  let mut staged_file = std::fs::File::open(staged.path().join("installer")).unwrap();
  let configured = iso::read(&mut staged_file, "boot/grub/grub.cfg", 65536, &check()).unwrap();
  assert!(configured.starts_with(boot::CONFIGURATION.as_bytes()));
  assert!(iso::read(&mut staged_file, "casper/vmlinuz", 4096, &check()).unwrap() == image());
  let owned = staged.path().to_owned();
  drop(staged);
  assert!(!owned.exists());
}

#[test]
fn decoding_checks_arm64_architecture_compression_and_payload_bounds() {
  let raw = image();
  let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
  gzip.write_all(&raw).unwrap();
  let compressed = gzip.finish().unwrap();
  let mut efi = vec![0; 64];
  efi[..8].copy_from_slice(b"MZ\0\0zimg");
  efi[8..12].copy_from_slice(&64u32.to_le_bytes());
  efi[12..16].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
  efi[24..29].copy_from_slice(b"gzip\0");
  efi.extend(compressed);
  assert!(kernel::decode(&efi, &check()).unwrap() == raw);
  let compressed = zstd::stream::encode_all(raw.as_slice(), 1).unwrap();
  efi.truncate(64);
  efi[12..16].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
  efi[24..29].copy_from_slice(b"zstd\0");
  efi.extend(compressed);
  assert!(kernel::decode(&efi, &check()).unwrap() == raw);
  assert!(kernel::decode(&efi[..efi.len() - 1], &check()).is_err());
  let mut wrong = image();
  wrong[24] = 1;
  assert!(kernel::decode(&wrong, &check()).is_err());
  let denied: Check = Arc::new(|| anyhow::bail!("revoked"));
  assert!(kernel::decode(&efi, &denied).is_err());
}

#[test]
fn corrupt_iso_extents_and_revocation_preserve_source() {
  let root = tempfile::tempdir().unwrap();
  let media = fixture::media(root.path());
  let original = std::fs::read(&media).unwrap();
  let denied: Check = Arc::new(|| anyhow::bail!("revoked"));
  assert!(boot::stage(&media, root.path(), &denied).is_err());
  assert!(std::fs::read(&media).unwrap() == original);
  let mut corrupt = original;
  corrupt[16 * 2048 + 84] ^= 1;
  assert!(iso::read(&mut Cursor::new(corrupt), "casper/vmlinuz", 4096, &check()).is_err());
}

#[tokio::test]
async fn unattended_preparation_cleans_staging_and_preserves_written_disks() {
  use engine::machines::{
    linux::{provision, records},
    vz::{self, Service, Stage},
    Actor, Machines,
  };
  use model::{CreateMachine, EngineResources};
  use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
  keyring::set_default_credential_builder(Box::new(backend::Builder(Arc::new(
    backend::State::default(),
  ))));
  let root = tempfile::tempdir().unwrap();
  let media = fixture::media(root.path());
  let original = std::fs::read(&media).unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let record = records::create(
    &manager,
    CreateMachine {
      name: "Unattended fixture".into(),
      profile: "ubuntu".into(),
      resources: EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 32,
      },
      agent_access: true,
      installer: Some(media.to_str().unwrap().into()),
    },
  )
  .unwrap();
  let (client, _) = vz::channel();
  let service = Service::new(manager.clone(), client);
  let prepared = service
    .prepare_linux(&record.id, Actor::Agent, Stage::Unattended)
    .await
    .unwrap();
  assert!(std::fs::read(&media).unwrap() == original);
  assert_eq!(
    std::fs::read_dir(manager.root.join("vz")).unwrap().count(),
    3
  );
  drop(prepared);
  assert_eq!(
    std::fs::read_dir(manager.root.join("vz")).unwrap().count(),
    0
  );
  let prepared = service
    .prepare_linux(&record.id, Actor::Agent, Stage::Unattended)
    .await
    .unwrap();
  manager.set_agent_access(&record.id, false).unwrap();
  let plan = provision::Plan {
    user_data: "#cloud-config\n".into(),
    meta_data: "owned".into(),
  };
  let error = prepared.provision(&plan).err().unwrap();
  assert!(error.to_string().contains("Agent access is off"));
  assert_eq!(
    std::fs::read_dir(manager.root.join("vz")).unwrap().count(),
    0
  );
  let target = manager.root.join("vz").join(&record.id);
  std::fs::create_dir(&target).unwrap();
  std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
  let mut disk = std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(target.join("disk"))
    .unwrap();
  disk.set_len(32 << 30).unwrap();
  disk.write_all(b"preserved installation data").unwrap();
  disk.sync_all().unwrap();
  for (name, data) in [
    ("identity", serde_json::to_vec(&serde_json::json!({"id":record.id,"version":1,"guest":"linux","diskGib":32,"identity":[1]})).unwrap()),
    ("variables", vec![1]),
  ] {
    let mut file = std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(target.join(name)).unwrap();
    file.write_all(&data).unwrap();
  }
  let error = service
    .prepare_linux(&record.id, Actor::Person, Stage::Unattended)
    .await
    .err()
    .unwrap();
  assert!(error.to_string().contains("contains data"));
  let mut disk = std::fs::File::open(target.join("disk")).unwrap();
  let mut content = [0; 27];
  std::io::Read::read_exact(&mut disk, &mut content).unwrap();
  assert_eq!(&content, b"preserved installation data");
  assert!(std::fs::read(&media).unwrap() == original);
}
