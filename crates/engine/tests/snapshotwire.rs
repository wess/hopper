#![cfg(unix)]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{native, Actor};
use fixture::{Fixture, ID};
use model::native::{Command, Result as Reply};
use std::time::Duration;

const SNAPSHOT: &str = "02020202-0202-0202-0202-020202020202";

#[tokio::test]
async fn public_session_requests_cannot_forward_private_snapshot_commands() {
  let f = Fixture::new();
  f.start("normal").await;
  for command in [
    Command::StartRestored {
      boot: f.boot("normal"),
      restore: model::native::Restore {
        snapshot_id: SNAPSHOT.into(),
        generation_id: SNAPSHOT.into(),
        policy_generation: 0,
      },
    },
    Command::StartGeneration {
      boot: f.boot("normal"),
      restore: model::native::Restore {
        snapshot_id: SNAPSHOT.into(),
        generation_id: SNAPSHOT.into(),
        policy_generation: 0,
      },
    },
    Command::RestoreResume {
      restore: model::native::Restore {
        snapshot_id: SNAPSHOT.into(),
        generation_id: SNAPSHOT.into(),
        policy_generation: 0,
      },
    },
    Command::RestorePause {
      restore: model::native::Restore {
        snapshot_id: SNAPSHOT.into(),
        generation_id: SNAPSHOT.into(),
        policy_generation: 0,
      },
    },
    Command::SnapshotStart {
      snapshot_id: SNAPSHOT.into(),
      policy_generation: 0,
    },
    Command::SnapshotPoll {
      snapshot_id: SNAPSHOT.into(),
      policy_generation: 0,
    },
    Command::SnapshotCancel {
      snapshot_id: SNAPSHOT.into(),
      policy_generation: 0,
    },
  ] {
    assert!(f.sessions.request(ID, Actor::Agent, command).await.is_err());
  }
  assert_eq!(f.trace(), "");
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn private_snapshot_replies_require_matching_identity_and_fixed_receipts() {
  for mode in ["normal", "snapshotforeign", "snapshotmalformed"] {
    let f = Fixture::new();
    let client = native::launch(&f.helper, f.boot(mode)).await.unwrap();
    let reply = native::request(
      &client,
      Command::SnapshotPoll {
        snapshot_id: SNAPSHOT.into(),
        policy_generation: 0,
      },
    )
    .await;
    if mode == "normal" {
      assert!(
        matches!(reply.unwrap(), Reply::Snapshot { snapshot_id, .. } if snapshot_id == SNAPSHOT)
      );
      native::request(&client, Command::Stop {}).await.unwrap();
    } else {
      assert!(reply.is_err());
    }
    tokio::time::timeout(Duration::from_secs(3), native::finished(&client))
      .await
      .unwrap()
      .unwrap();
  }
}
