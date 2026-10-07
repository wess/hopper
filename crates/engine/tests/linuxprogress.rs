#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::linux::{
  observe,
  progress::{self, Decoder, Phase},
  provision,
};
use std::{
  io::Write,
  os::unix::fs::PermissionsExt,
  time::{Duration, Instant},
};

const ATTEMPT: &str = "8197e0f0-0603-43e9-a817-eaf7ab0327af";

fn marker(phase: &str) -> String {
  format!("HOPPER-INSTALL:{ATTEMPT}:{phase}\n")
}

#[test]
fn decoder_is_bounded_scoped_ordered_and_survives_fragmentation() {
  let mut decoder = Decoder::new(ATTEMPT).unwrap();
  assert!(decoder.feed(marker("deployed").as_bytes()).is_empty());
  assert!(decoder
    .feed(
      marker("installing")
        .replace(ATTEMPT, "another-attempt")
        .as_bytes()
    )
    .is_empty());
  assert!(decoder.feed(&vec![b'x'; 1 << 20]).is_empty());
  assert!(decoder.feed(marker("installing").as_bytes()).is_empty());
  assert_eq!(decoder.phase(), Phase::Prepared);
  let mut changes = Vec::new();
  for byte in marker("installing").replace("\n", "\r\n").bytes() {
    changes.extend(decoder.feed(&[byte]));
  }
  assert_eq!(changes, [Phase::Installing]);
  assert_eq!(
    decoder.feed(marker("deployed").as_bytes()),
    [Phase::Deployed]
  );
  assert!(decoder.feed(marker("installing").as_bytes()).is_empty());
  assert_eq!(decoder.feed(marker("failed").as_bytes()), [Phase::Failed]);
  assert!(decoder.feed(marker("deployed").as_bytes()).is_empty());
  assert!(Decoder::new("bad; rm -rf /").is_err());
}

#[test]
fn pipe_drains_noise_and_persists_only_installation_phases() {
  let root = tempfile::tempdir().unwrap();
  std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
  let (mut output, observation) = observe::start(root.path(), ATTEMPT).unwrap();
  assert_eq!(progress::read(root.path()).unwrap(), Some(Phase::Prepared));
  output.write_all(&vec![b'x'; 1 << 20]).unwrap();
  output.write_all(b"\n").unwrap();
  output.write_all(marker("installing").as_bytes()).unwrap();
  output.write_all(marker("deployed").as_bytes()).unwrap();
  let deadline = Instant::now() + Duration::from_secs(5);
  while progress::read(root.path()).unwrap() != Some(Phase::Deployed) {
    assert!(
      Instant::now() < deadline,
      "Installation observer did not persist completion"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  let record = root.path().join("installation");
  assert_eq!(
    std::fs::metadata(&record).unwrap().permissions().mode() & 0o777,
    0o600
  );
  assert!(std::fs::metadata(&record).unwrap().len() < 256);
  drop(output);
  drop(observation);
  std::fs::write(&record, b"{corrupt}").unwrap();
  assert!(progress::read(root.path()).is_err());
}

#[test]
fn seed_hooks_execute_real_shell_markers_without_passwords() {
  let credentials = provision::accounts();
  let plan = provision::tracked(ATTEMPT, &credentials, ATTEMPT).unwrap();
  let data: serde_json::Value = serde_yaml::from_str(&plan.user_data).unwrap();
  let mut decoder = Decoder::new(ATTEMPT).unwrap();
  for (key, expected) in [
    ("early-commands", Phase::Installing),
    ("late-commands", Phase::Deployed),
    ("error-commands", Phase::Failed),
  ] {
    let command = data["autoinstall"][key][0].as_str().unwrap();
    assert!(!command.contains(&credentials.user_password));
    assert!(!command.contains(&credentials.administrator_password));
    let command = command.strip_suffix(" > /dev/hvc0").unwrap();
    let result = std::process::Command::new("/bin/sh")
      .args(["-c", command])
      .output()
      .unwrap();
    assert!(result.status.success());
    assert_eq!(decoder.feed(&result.stdout), [expected]);
  }
}

#[test]
fn installation_status_rechecks_agent_access_and_preserves_corrupt_journals() {
  use engine::machines::{linux::records, vz, Actor, Machines};
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let record = records::create(
    &manager,
    model::CreateMachine {
      name: "Progress fixture".into(),
      profile: "ubuntu".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 32,
      },
      agent_access: true,
      installer: None,
    },
  )
  .unwrap();
  let (client, _) = vz::channel();
  let service = vz::Service::new(manager.clone(), client);
  assert_eq!(
    service.installation(&record.id, Actor::Agent).unwrap(),
    None
  );
  let directory = manager.root.join("vz").join(&record.id);
  std::fs::create_dir_all(&directory).unwrap();
  std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
  let (output, observation) = observe::start(&directory, ATTEMPT).unwrap();
  assert_eq!(
    service.installation(&record.id, Actor::Agent).unwrap(),
    Some(Phase::Prepared)
  );
  manager.set_agent_access(&record.id, false).unwrap();
  assert!(service.installation(&record.id, Actor::Agent).is_err());
  assert_eq!(
    service.installation(&record.id, Actor::Person).unwrap(),
    Some(Phase::Prepared)
  );
  drop(output);
  drop(observation);
  let path = directory.join("installation");
  std::fs::write(&path, b"{broken}").unwrap();
  assert!(service.installation(&record.id, Actor::Person).is_err());
  assert_eq!(std::fs::read(&path).unwrap(), b"{broken}");
}

#[test]
fn reader_teardown_drains_a_final_failure_without_retaining_console_logs() {
  let root = tempfile::tempdir().unwrap();
  std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
  let (mut output, observation) = observe::start(root.path(), ATTEMPT).unwrap();
  output
    .write_all(
      format!(
        "{}{}{}",
        marker("installing"),
        marker("deployed"),
        marker("failed")
      )
      .as_bytes(),
    )
    .unwrap();
  drop(output);
  drop(observation);
  let deadline = Instant::now() + Duration::from_secs(5);
  while progress::read(root.path()).unwrap() != Some(Phase::Failed) {
    assert!(
      Instant::now() < deadline,
      "Teardown lost a buffered failure marker"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
