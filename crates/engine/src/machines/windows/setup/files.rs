use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  collections::BTreeMap,
  io::{Read, Write},
  path::Path,
};

pub(crate) const DRIVERS: &[(&str, &[&str])] = &[
  ("viostor", &["viostor.inf", "viostor.cat", "viostor.sys"]),
  ("vioscsi", &["vioscsi.inf", "vioscsi.cat", "vioscsi.sys"]),
  (
    "vioinput",
    &[
      "vioinput.inf",
      "vioinput.cat",
      "vioinput.sys",
      "viohidkmdf.sys",
    ],
  ),
  ("vioserial", &["vioser.inf", "vioser.cat", "vioser.sys"]),
];

pub(super) fn drivers(
  root: &Path,
  license: &Path,
  payload: &Path,
) -> anyhow::Result<BTreeMap<String, String>> {
  let mut hashes = BTreeMap::new();
  for (driver, files) in DRIVERS {
    std::fs::create_dir(payload.join(driver))?;
    for name in *files {
      let source = root.join(driver).join("w11/ARM64").join(name);
      let bytes = read(&source, 8 * 1024 * 1024)?;
      validate(name, &bytes)?;
      write(&payload.join(driver).join(name), &bytes)?;
      hashes.insert(
        format!("{driver}/{name}"),
        format!("{:x}", Sha256::digest(&bytes)),
      );
    }
  }
  write(&payload.join("license.txt"), &read(license, 1024 * 1024)?)?;
  Ok(hashes)
}

pub fn validate(name: &str, bytes: &[u8]) -> anyhow::Result<()> {
  ensure!(!bytes.is_empty(), "Empty guest driver file");
  if name.ends_with(".sys") {
    ensure!(
      bytes.len() >= 64 && &bytes[..2] == b"MZ",
      "Invalid driver executable"
    );
    let offset = u32::from_le_bytes(bytes[60..64].try_into()?) as usize;
    ensure!(
      offset.checked_add(6).is_some_and(|end| end <= bytes.len()),
      "Invalid driver PE offset"
    );
    ensure!(
      &bytes[offset..offset + 4] == b"PE\0\0"
        && u16::from_le_bytes(bytes[offset + 4..offset + 6].try_into()?) == 0xaa64,
      "Driver is not ARM64"
    );
  } else if name.ends_with(".inf") {
    ensure!(
      bytes
        .windows(7)
        .any(|part| part.eq_ignore_ascii_case(b"NTARM64")),
      "Driver INF has no ARM64 section"
    );
  }
  Ok(())
}

pub(super) fn read(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
  let info = std::fs::symlink_metadata(path)?;
  ensure!(
    info.is_file() && (1..=limit).contains(&info.len()),
    "Invalid setup input file"
  );
  let mut options = std::fs::OpenOptions::new();
  options.read(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
  }
  let file = options.open(path)?;
  ensure!(
    file.metadata()?.is_file(),
    "Setup input changed while opening"
  );
  let mut bytes = Vec::new();
  file.take(limit + 1).read_to_end(&mut bytes)?;
  ensure!(
    !bytes.is_empty() && bytes.len() as u64 <= limit,
    "Setup input exceeds its bound"
  );
  Ok(bytes)
}

pub(super) fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
  let mut options = std::fs::OpenOptions::new();
  options.create_new(true).write(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  let mut file = options.open(path)?;
  file.write_all(bytes)?;
  file.sync_all()?;
  Ok(())
}

pub(crate) async fn digest(path: &Path, limit: u64) -> anyhow::Result<String> {
  let path = path.to_owned();
  tokio::task::spawn_blocking(move || {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?.take(limit + 1);
    let info = file.get_ref().metadata()?;
    ensure!(
      info.is_file() && (1..=limit).contains(&info.len()),
      "Invalid installer checksum input"
    );
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut total = 0u64;
    loop {
      let read = file.read(&mut buffer)?;
      if read == 0 {
        break;
      }
      total += read as u64;
      ensure!(total <= limit, "Installer exceeds its checksum bound");
      hash.update(&buffer[..read]);
    }
    ensure!(
      total == info.len(),
      "Installer size changed during checksum"
    );
    Ok(format!("{:x}", hash.finalize()))
  })
  .await?
}
