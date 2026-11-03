#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::native::{self, config, restore::Owner};
use fixture::{Fixture, ID};
use machine::{
  runtime::hardware,
  snapshot::{
    manifest::{Binding, Layout},
    package::Stage,
  },
};
use model::native::Boot;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const SOURCE: &str = "02020202-0202-0202-0202-020202020202";
const PRIOR: &str = "05050505-0505-0505-0505-050505050505";

fn locked(f: &Fixture) {
  for name in [ID.to_owned(), format!("{ID}.runtime")] {
    let file = fs::OpenOptions::new()
      .read(true)
      .write(true)
      .open(f.manager.root.join("locks").join(name))
      .unwrap();
    let error = store::lock::exclusive(file)
      .err()
      .expect("retained VM lease");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
  }
}

fn fixture() -> (Fixture, Boot) {
  let f = Fixture::new();
  let paths = f.initialize();
  let boot = Boot {
    machine_id: ID.into(),
    firmware: f.firmware.join("windows.fd").to_string_lossy().into(),
    variables: f.firmware.join("variables.fd").to_string_lossy().into(),
    store: paths.variables.to_string_lossy().into(),
    disk: Some(paths.disk.to_string_lossy().into()),
    disk_id: "hopper00000000000001".into(),
    cpus: 2,
    memory: 2 << 30,
    boot_media: None,
    installer: None,
    timeout_ms: None,
  };
  (f, boot)
}

fn invalid_packages(f: &Fixture, boot: &Boot) {
  let root = PathBuf::from(&boot.store).parent().unwrap().to_owned();
  let parent = fixture::private(&root.join("snapshots"));
  let (_, devices, configuration) = hardware::original(boot.clone(), || Ok(())).unwrap();
  let layout = Layout {
    cpus: 2,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, 2 << 30),
    ],
    devices: [0, 2, 3, 3, 1, 0, 5],
  };
  for snapshot in [SOURCE, PRIOR] {
    let mut stage = Stage::new(
      &parent,
      Binding {
        machine: model::native::machine_identity(ID).unwrap(),
        snapshot: model::native::machine_identity(snapshot).unwrap(),
        configuration,
      },
      layout,
      || Ok(()),
    )
    .unwrap();
    // complete, checksummed inventory with intentionally invalid component schemas.
    let mut bytes = Vec::from(b"invalid decoder");
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    for component in layout.records().unwrap() {
      stage
        .write(component, || Ok(()), |out| Ok(out.write_all(&bytes)?))
        .unwrap();
    }
    drop(stage.publish(|| Ok(()), || Ok(())).unwrap());
  }
  drop(devices);
  assert!(f
    .manager
    .machine(ID, engine::machines::Actor::Person)
    .is_ok());
}

#[tokio::test]
async fn invalid_prior_components_reject_before_generation_or_selection_publication() {
  let (f, boot) = fixture();
  invalid_packages(&f, &boot);
  let paths = config::paths(&f.manager.root, ID).unwrap();
  let original = fs::metadata(&paths.disk).unwrap();
  let owner = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  assert!(native::restore::prepare(owner, boot, SOURCE, PRIOR)
    .await
    .is_err());
  assert!(!paths.root.join("restore.hrr").exists());
  assert!(!paths.root.join("generations").exists());
  assert!(!paths.variables.exists());
  assert_eq!(
    fs::metadata(&paths.disk).unwrap().modified().unwrap(),
    original.modified().unwrap()
  );
  assert_eq!(
    fs::read_dir(paths.root.join("snapshots")).unwrap().count(),
    2
  );
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn original_policy_and_distinct_prior_are_required_before_preparation() {
  let (f, boot) = fixture();
  let owner = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  assert!(
    native::restore::prepare(owner, boot.clone(), SOURCE, SOURCE)
      .await
      .is_err()
  );
  let owner = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(native::restore::prepare(owner, boot, SOURCE, PRIOR)
    .await
    .is_err());
  let paths = config::paths(&f.manager.root, ID).unwrap();
  assert!(!paths.root.join("generations").exists());
  assert!(!paths.root.join("restore.hrr").exists());
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn prepared_handoff_or_foreign_boot_cannot_be_reprepared() {
  let (f, boot, _, owner) = fixture::worker("restored");
  let paths = config::paths(&f.manager.root, ID).unwrap();
  let receipt = fs::read(paths.root.join("restore.hrr")).unwrap();
  assert!(native::restore::prepare(owner, boot, SOURCE, PRIOR)
    .await
    .is_err());
  assert_eq!(fs::read(paths.root.join("restore.hrr")).unwrap(), receipt);
  let (f, mut boot) = fixture();
  let owner = Owner::person(f.manager.clone(), ID).unwrap();
  boot.store = f
    .root
    .path()
    .join("foreign/variables")
    .to_string_lossy()
    .into();
  assert!(native::restore::prepare(owner, boot, SOURCE, PRIOR)
    .await
    .is_err());
  assert!(!f.root.path().join("foreign").exists());
}

#[tokio::test]
async fn successor_rejects_invalid_components_without_changing_predecessor() {
  let (f, boot) = fixture();
  invalid_packages(&f, &boot);
  let (_, devices, configuration) = hardware::original(boot.clone(), || Ok(())).unwrap();
  drop(devices);
  let expected = native::restore::Record {
    configuration,
    phase: native::restore::Phase::Attempted,
    ..fixture::record()
  };
  let paths = config::paths(&f.manager.root, ID).unwrap();
  let bytes = expected.encode().unwrap();
  fs::write(paths.root.join("restore.hrr"), &bytes).unwrap();
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(
    paths.root.join("restore.hrr"),
    fs::Permissions::from_mode(0o400),
  )
  .unwrap();
  let owner = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  assert!(
    native::restore::prepare_successor(owner, boot, PRIOR, SOURCE, expected)
      .await
      .is_err()
  );
  assert_eq!(fs::read(paths.root.join("restore.hrr")).unwrap(), bytes);
  assert!(!paths.root.join("generations").exists());
  assert!(!paths
    .root
    .join(format!(
      "restore{}.hrr",
      fixture::identity(expected.generation)
    ))
    .exists());
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn successor_requires_exact_predecessor_before_hardware_or_storage_work() {
  let (f, boot, _, owner) = fixture::worker("restored");
  let paths = config::paths(&f.manager.root, ID).unwrap();
  let bytes = fs::read(paths.root.join("restore.hrr")).unwrap();
  // The selected receipt is Attempted; a stale Prepared receipt must not reach firmware reads.
  fs::remove_file(&boot.firmware).unwrap();
  let error = native::restore::prepare_successor(owner, boot, SOURCE, PRIOR, fixture::record())
    .await
    .err()
    .unwrap();
  assert!(error.to_string().contains("predecessor"));
  assert_eq!(fs::read(paths.root.join("restore.hrr")).unwrap(), bytes);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn failed_preparation_returns_original_owners_without_a_runtime_or_operation_gap() {
  let (f, boot) = fixture();
  invalid_packages(&f, &boot);
  let owner = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  let outcome = native::restore::prepare_retained(owner, boot, SOURCE, PRIOR, None)
    .await
    .unwrap();
  let native::restore::Preparation { owner, result } = outcome;
  let error = result.expect_err("invalid checkpoint rejected");
  assert!(!format!("{error:#}").is_empty());
  assert!(owner.selected().unwrap().is_none());
  locked(&f);
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  owner.check().unwrap();
  drop(owner);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn retained_failure_owner_does_not_refresh_revoked_agent_authorization() {
  let (f, boot) = fixture();
  let owner = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  let outcome = native::restore::prepare_retained(owner, boot, SOURCE, SOURCE, None)
    .await
    .unwrap();
  let native::restore::Preparation { owner, result } = outcome;
  let error = result.expect_err("identical checkpoints rejected");
  assert!(error.to_string().contains("distinct"));
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(owner.check().is_err());
  assert!(owner.selected().is_err());
  locked(&f);
  drop(owner);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn stale_predecessor_failure_retains_owner_and_preserves_the_original_selection() {
  let (f, boot, _, owner) = fixture::worker("restored");
  let paths = config::paths(&f.manager.root, ID).unwrap();
  let original = fs::read(paths.root.join("restore.hrr")).unwrap();
  fs::remove_file(&boot.firmware).unwrap();
  let outcome =
    native::restore::prepare_retained(owner, boot, SOURCE, PRIOR, Some(fixture::record()))
      .await
      .unwrap();
  let native::restore::Preparation { owner, result } = outcome;
  let error = result.expect_err("stale predecessor rejected");
  assert!(error.to_string().contains("predecessor"));
  assert_eq!(fs::read(paths.root.join("restore.hrr")).unwrap(), original);
  locked(&f);
  drop(owner);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}
