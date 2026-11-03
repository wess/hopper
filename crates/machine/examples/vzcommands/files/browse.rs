use anyhow::ensure;
use machine::vz::queue::{Check, Client};
use model::guest::{
  files::{Browse, Listing},
  wire::Action,
  Role,
};

pub(super) async fn check(client: &Client, id: &str, check: &Check) -> anyhow::Result<()> {
  let mut directory = Browse {
    path: "/home/hopper".into(),
    limit: 1,
    after: None,
    stamp: None,
    role: Role::Normal,
  };
  let mut names = Vec::new();
  let mut complete = false;
  for _ in 0..32 {
    let job = super::run(
      client,
      id,
      check,
      Action::Start {
        command: directory.command().unwrap(),
      },
    )
    .await?;
    ensure!(
      job.exit_code == Some(0)
        && job
          .identity
          .as_ref()
          .is_some_and(|identity| identity.uid == 1001),
      "Guest browse identity or completion mismatch"
    );
    let listing: Listing = serde_json::from_slice(&job.stdout)?;
    ensure!(
      listing.entries.len() <= 1 && listing.stamp.valid(),
      "Guest browse exceeded page bounds"
    );
    for entry in &listing.entries {
      ensure!(
        directory
          .after
          .as_ref()
          .is_none_or(|after| &entry.name > after),
        "Guest browse did not advance cursor"
      );
      names.push(entry.name.clone());
    }
    if let Some(next) = listing.next {
      ensure!(
        listing
          .entries
          .last()
          .is_some_and(|entry| entry.name == next),
        "Guest browse cursor mismatch"
      );
      directory.after = Some(next);
      directory.stamp = Some(listing.stamp);
    } else {
      ensure!(
        names.contains(&b"normal.upload".to_vec()) && names.contains(&b"admin.upload".to_vec()),
        "Guest browse omitted uploaded files"
      );
      complete = true;
      break;
    }
  }
  ensure!(complete, "Guest browse exceeded diagnostic page limit");
  for (path, role) in [
    ("/root", Role::Normal),
    ("/proc", Role::Administrator),
    ("/sys", Role::Administrator),
  ] {
    let directory = Browse {
      path: path.into(),
      limit: 32,
      after: None,
      stamp: None,
      role,
    };
    let job = super::run(
      client,
      id,
      check,
      Action::Start {
        command: directory.command().unwrap(),
      },
    )
    .await?;
    ensure!(
      job.exit_code.is_some_and(|code| code != 0) && job.stdout.is_empty(),
      "Guest directory restrictions exposed entries"
    );
  }
  Ok(())
}
