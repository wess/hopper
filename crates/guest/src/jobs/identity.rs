use anyhow::{ensure, Context};
use model::guest::{Identity, Role};
use std::{
  io::Read,
  os::unix::fs::{MetadataExt, OpenOptionsExt},
  path::Path,
};

fn document(path: &str) -> anyhow::Result<String> {
  let file = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(path)?;
  let info = file.metadata()?;
  ensure!(
    info.is_file() && info.uid() == 0 && info.mode() & 0o022 == 0,
    "Guest identity database is not protected"
  );
  let mut text = String::new();
  file.take((128 << 10) + 1).read_to_string(&mut text)?;
  ensure!(
    text.len() <= 128 << 10,
    "Guest identity database exceeds bounds"
  );
  Ok(text)
}

pub(super) fn resolve(role: Role) -> anyhow::Result<(Identity, String)> {
  let user = if role == Role::Administrator {
    "root"
  } else {
    "hopper"
  };
  let passwd = document("/etc/passwd")?;
  let entries: Vec<_> = passwd
    .lines()
    .filter(|line| line.split(':').next() == Some(user))
    .collect();
  ensure!(entries.len() == 1, "Guest account is absent or ambiguous");
  let fields: Vec<_> = entries[0].split(':').collect();
  ensure!(fields.len() == 7, "Guest account record is malformed");
  let uid: u32 = fields[2].parse()?;
  let gid: u32 = fields[3].parse()?;
  ensure!(
    (uid == 0) == (role == Role::Administrator),
    "Guest account privilege does not match the requested role"
  );
  let home = fields[5].to_owned();
  ensure!(
    Path::new(&home).is_absolute() && home.len() <= 4096,
    "Guest home directory is invalid"
  );
  let mut groups = vec![gid];
  for line in document("/etc/group")?.lines() {
    let fields: Vec<_> = line.split(':').collect();
    ensure!(fields.len() == 4, "Guest group record is malformed");
    if fields[3].split(',').any(|member| member == user) {
      let group = fields[2].parse().context("Guest group ID is invalid")?;
      if !groups.contains(&group) {
        groups.push(group);
      }
    }
  }
  ensure!(
    groups.len() <= 64,
    "Guest supplementary groups exceed bounds"
  );
  Ok((
    Identity {
      user: user.into(),
      uid,
      gid,
      groups,
      administrator: uid == 0,
    },
    home,
  ))
}
