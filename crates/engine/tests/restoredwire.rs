#![cfg(unix)]
#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../src/machines/native/wire.rs"]
mod wire;
use model::native::{Command, Response, Restore, Result as Reply};

#[test]
fn paused_activation_never_accepts_a_running_reply() {
  let restore = Restore {
    snapshot_id: "02020202-0202-0202-0202-020202020202".into(),
    generation_id: "04040404-0404-0404-0404-040404040404".into(),
    policy_generation: 7,
  };
  for paused in [false, true] {
    let command = if paused {
      Command::RestorePause {
        restore: restore.clone(),
      }
    } else {
      Command::RestoreResume {
        restore: restore.clone(),
      }
    };
    for reply_paused in [false, true] {
      let response = Response {
        id: 1,
        result: if reply_paused {
          Reply::Paused {}
        } else {
          Reply::Running {}
        },
      };
      assert_eq!(
        wire::validate(1, &command, &response).is_ok(),
        paused == reply_paused
      );
    }
  }
}

#[test]
fn held_restored_reply_requires_original_vm_checkpoint_generation_and_policy() {
  held(false);
}

#[test]
fn held_generation_reply_requires_original_bindings_and_distinct_boot_kind() {
  held(true);
}

fn held(system: bool) {
  let f = fixture::Fixture::new();
  let restore = Restore {
    snapshot_id: "02020202-0202-0202-0202-020202020202".into(),
    generation_id: "04040404-0404-0404-0404-040404040404".into(),
    policy_generation: 7,
  };
  let command = if system {
    Command::StartGeneration {
      boot: f.boot("normal"),
      restore: restore.clone(),
    }
  } else {
    Command::StartRestored {
      boot: f.boot("normal"),
      restore: restore.clone(),
    }
  };
  for fault in 0..7 {
    let mut machine_id = fixture::ID.to_owned();
    let mut returned = restore.clone();
    let mut configuration = [3; 32];
    match fault {
      1 => machine_id = "09090909-0909-0909-0909-090909090909".into(),
      2 => returned.snapshot_id = returned.generation_id.clone(),
      3 => returned.generation_id = returned.snapshot_id.clone(),
      4 => returned.policy_generation += 1,
      5 => configuration = [0; 32],
      6 => machine_id = "../other".into(),
      _ => {}
    }
    let response = Response {
      id: 1,
      result: if system {
        Reply::GenerationStarted {
          machine_id,
          configuration,
          restore: returned,
        }
      } else {
        Reply::Restored {
          machine_id,
          configuration,
          restore: returned,
        }
      },
    };
    assert_eq!(wire::validate(1, &command, &response).is_ok(), fault == 0);
    assert!(wire::validate(2, &command, &response).is_err());
  }
  let wrong_kind = Response {
    id: 1,
    result: if system {
      Reply::Restored {
        machine_id: fixture::ID.into(),
        configuration: [3; 32],
        restore: restore.clone(),
      }
    } else {
      Reply::GenerationStarted {
        machine_id: fixture::ID.into(),
        configuration: [3; 32],
        restore: restore.clone(),
      }
    },
  };
  assert!(wire::validate(1, &command, &wrong_kind).is_err());
  let ordinary = Response {
    id: 1,
    result: Reply::Started {
      machine_id: fixture::ID.into(),
      configuration: [3; 32],
    },
  };
  assert!(wire::validate(1, &command, &ordinary).is_err());
}
