use super::fixture;
use engine::machines::{
  native::{
    config::{self, Stage},
    restore::{self, Journal, Phase},
    snapshot::Policy,
  },
  Machines,
};
use fixture::{Fixture, ID};
use std::{fs::File, os::unix::fs::FileExt, process::Command};

#[test]
fn committed_state_survives_writer_exit_and_excludes_another_host() {
  let f = Fixture::new();
  let paths = f.initialize();
  fixture::artifacts(&paths.root);
  std::fs::OpenOptions::new()
    .write(true)
    .open(&paths.disk)
    .unwrap()
    .write_all_at(b"boot", 0)
    .unwrap();
  config::prepare(&f.manager, ID, &f.assets(), Stage::System).unwrap();
  let invoke = |mode| {
    Command::new(std::env::current_exe().unwrap())
      .args(["--exact", "process::restore_writer", "--ignored"])
      .env("HOPPER_RESTORE_ROOT", &f.manager.root)
      .env("HOPPER_RESTORE_MODE", mode)
      .status()
      .unwrap()
      .code()
  };
  assert_eq!(invoke("select"), Some(7));
  let root = File::open(&paths.root).unwrap();
  let record = fixture::record();
  assert_eq!(restore::read(&root, record.machine).unwrap(), Some(record));
  let journal = Journal::open(&root, record.machine).unwrap();
  assert_eq!(invoke("busy"), Some(9));
  drop(journal);
  assert_eq!(invoke("attempt"), Some(7));
  assert_eq!(
    restore::read(&root, record.machine).unwrap().unwrap().phase,
    Phase::Attempted
  );
  drop(Journal::open(&root, record.machine).unwrap());
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System)
    .err()
    .unwrap()
    .to_string()
    .contains("restore"));
}

#[test]
#[ignore = "invoked by committed_state_survives_writer_exit_and_excludes_another_host"]
fn restore_writer() {
  let manager = Machines {
    root: std::env::var_os("HOPPER_RESTORE_ROOT").unwrap().into(),
  };
  let record = fixture::record();
  let root = File::open(config::paths(&manager.root, ID).unwrap().root).unwrap();
  let mode = std::env::var("HOPPER_RESTORE_MODE").unwrap();
  if mode == "busy" {
    assert!(Journal::open(&root, record.machine).is_err());
    std::process::exit(9);
  }
  let policy = Policy::agent(manager, ID, 0).unwrap();
  let mut journal = Journal::open(&root, record.machine).unwrap();
  match mode.as_str() {
    "select" => journal
      .select(record, fixture::layout(), &policy, || Ok(()))
      .unwrap(),
    "attempt" => {
      journal.attempted(record, &policy, || Ok(())).unwrap();
    }
    _ => panic!("unknown restore writer mode"),
  }
  // no destructors: the following reader must rely on durable files and OS lock release.
  std::process::exit(7);
}
