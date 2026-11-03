#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{
    config,
    restore::{self, Owner, Phase, Record},
    sessions::Sessions,
  },
  Machines,
};
use fixture::ID;
use std::{fs, path::PathBuf, time::Duration};

// synthetic worker acknowledgement; this exercises host durability, not guest checkpoint admission.
#[test]
#[ignore]
fn activation_crash_child() {
  let output = std::env::var_os("HOPPER_ACTIVE_CRASH_FILE").expect("parent fixture");
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  runtime.block_on(async {
    let (f, boot, restore, owner) = fixture::worker("restored");
    let sessions = Sessions::new(f.manager.clone());
    sessions
      .start_restored(ID, &f.helper, boot, restore, owner)
      .await
      .unwrap();
    fs::write(output, f.root.path().to_string_lossy().as_bytes()).unwrap();
    // no registry, worker, lease or temporary-directory destructor runs.
    std::process::exit(7);
  });
}

#[tokio::test]
async fn active_generation_survives_host_process_exit_and_never_falls_back() {
  let parent = tempfile::tempdir().unwrap();
  let output = parent.path().join("profile");
  let status = std::process::Command::new(std::env::current_exe().unwrap())
    .args(["--ignored", "--exact", "activation_crash_child"])
    .env("HOPPER_ACTIVE_CRASH_FILE", &output)
    .status()
    .unwrap();
  assert_eq!(status.code(), Some(7));
  let profile = PathBuf::from(fs::read_to_string(&output).unwrap());
  let manager = Machines {
    root: profile.join("machines"),
  };
  let paths = config::paths(&manager.root, ID).unwrap();
  let pid: i32 = fs::read_to_string(paths.variables.join("pid"))
    .unwrap()
    .parse()
    .unwrap();
  tokio::time::timeout(Duration::from_secs(4), async {
    while unsafe { libc::kill(pid, 0) == 0 } {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .expect("orphan worker did not observe parent EOF");
  let expected = Record {
    phase: Phase::Active,
    ..fixture::record()
  };
  assert_eq!(
    restore::read(&fs::File::open(&paths.root).unwrap(), expected.machine).unwrap(),
    Some(expected)
  );
  let owner = Owner::person(manager.clone(), ID).unwrap();
  assert_eq!(owner.selected().unwrap(), Some(expected));
  assert!(config::initialize(&manager, ID).is_err());
  assert!(paths
    .root
    .join("generations")
    .join(fixture::identity(expected.generation))
    .exists());
  assert!(paths
    .root
    .join("snapshots")
    .join(fixture::identity(expected.rollback))
    .exists());
  drop(owner);
  fs::remove_dir_all(profile).unwrap();
}
