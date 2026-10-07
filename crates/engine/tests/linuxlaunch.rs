#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::{io::Write, sync::Arc};

#[path = "support/credentials.rs"]
mod backend;
#[path = "support/linux.rs"]
mod fixture;

#[tokio::test]
async fn automatic_launch_preserves_partial_disks_and_reuses_completed_systems() {
  use engine::machines::{
    linux::{
      observe,
      progress::{self, Phase},
      records,
    },
    vz::{self, Service, Stage},
    Actor, Machines,
  };
  use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    time::{Duration, Instant},
  };
  keyring::set_default_credential_builder(Box::new(backend::Builder(Arc::new(
    backend::State::default(),
  ))));
  let root = tempfile::tempdir().unwrap();
  let media = fixture::media(root.path());
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let record = records::create(
    &manager,
    model::CreateMachine {
      name: "Launch fixture".into(),
      profile: "ubuntu".into(),
      resources: model::EngineResources {
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
  let (report, status) =
    tokio::sync::watch::channel(engine::machines::linux::preparation::Phase::Inspecting);
  let prepared = service
    .prepare_linux_tracked(&record.id, Actor::Agent, Stage::Launch, report)
    .await
    .unwrap();
  assert_eq!(prepared.stage(), Stage::Unattended);
  assert_eq!(
    *status.borrow(),
    engine::machines::linux::preparation::Phase::Ready
  );
  drop(prepared);
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
  disk.write_all(b"preserved partial system disk").unwrap();
  disk.sync_all().unwrap();
  for (name, data) in [
    ("identity", serde_json::to_vec(&serde_json::json!({"id":record.id,"version":1,"guest":"linux","diskGib":32,"identity":[1]})).unwrap()),
    ("variables", vec![1]),
  ] {
    let mut file = std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(target.join(name)).unwrap();
    file.write_all(&data).unwrap();
  }
  let attempt = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  let (mut output, observation) = observe::start(&target, attempt).unwrap();
  for phase in [Phase::Prepared, Phase::Installing, Phase::Deployed] {
    match phase {
      Phase::Prepared => {}
      Phase::Installing => writeln!(output, "HOPPER-INSTALL:{attempt}:installing").unwrap(),
      Phase::Deployed => writeln!(output, "HOPPER-INSTALL:{attempt}:deployed").unwrap(),
      _ => unreachable!(),
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while progress::read(&target).unwrap() != Some(phase) {
      assert!(Instant::now() < deadline);
      std::thread::sleep(Duration::from_millis(10));
    }
    let result = service
      .prepare_linux(&record.id, Actor::Agent, Stage::Launch)
      .await;
    if phase == Phase::Deployed {
      assert_eq!(result.unwrap().stage(), Stage::System);
    } else {
      assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("requires recovery"));
    }
  }
  drop(output);
  drop(observation);
  std::fs::remove_file(&media).unwrap();
  assert_eq!(
    service
      .prepare_linux(&record.id, Actor::Person, Stage::Launch)
      .await
      .unwrap()
      .stage(),
    Stage::System
  );
  let mut disk = std::fs::File::open(target.join("disk")).unwrap();
  let mut content = [0; 29];
  std::io::Read::read_exact(&mut disk, &mut content).unwrap();
  assert_eq!(&content, b"preserved partial system disk");
  manager.set_agent_access(&record.id, false).unwrap();
  assert!(service
    .prepare_linux(&record.id, Actor::Agent, Stage::Launch)
    .await
    .is_err());
}
