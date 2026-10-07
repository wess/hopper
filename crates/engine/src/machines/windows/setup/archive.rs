use super::process;
use anyhow::ensure;
use std::{
  collections::HashSet,
  path::{Component, Path, PathBuf},
};

pub fn members(list: &str) -> anyhow::Result<Vec<(String, PathBuf)>> {
  let mut names = HashSet::new();
  let mut result = Vec::new();
  for name in list.lines() {
    if name.ends_with('/') {
      continue;
    }
    let lower = name.to_ascii_lowercase();
    if !(lower.starts_with("efi/")
      || lower.starts_with("boot/")
      || lower == "sources/boot.wim"
      || lower == "bootmgr.efi")
    {
      continue;
    }
    ensure!(
      name.is_ascii()
        && name
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || b" /._-".contains(&byte)),
      "Unsupported Windows boot member name"
    );
    let path = Path::new(&lower);
    ensure!(
      path
        .components()
        .all(|part| matches!(part, Component::Normal(_))),
      "Unsafe Windows boot member path"
    );
    ensure!(
      names.insert(lower.clone()),
      "Ambiguous Windows boot member path"
    );
    ensure!(result.len() < 2048, "Too many Windows boot members");
    result.push((name.to_owned(), PathBuf::from(lower)));
  }
  ensure!(
    names.contains("sources/boot.wim") && names.contains("efi/microsoft/boot/efisys.bin"),
    "Installer has no Windows PE/UEFI boot media"
  );
  Ok(result)
}

pub fn catalogue(list: &str, metadata: &str) -> anyhow::Result<Vec<(String, PathBuf)>> {
  let names: Vec<_> = list.lines().collect();
  let records: Vec<_> = metadata.lines().collect();
  ensure!(
    names.len() == records.len(),
    "Inconsistent Windows archive listing"
  );
  let regular = names
    .iter()
    .zip(records)
    .filter_map(|(name, record)| record.starts_with('-').then_some(*name))
    .collect::<Vec<_>>()
    .join("\n");
  members(&regular)
}

pub(super) async fn extract(tool: &Path, installer: &Path, output: &Path) -> anyhow::Result<u64> {
  let source = installer
    .to_str()
    .ok_or_else(|| anyhow::anyhow!("Installer path is not UTF-8"))?
    .to_owned();
  let listing = process::run(
    tool,
    &["-tf".into(), source.clone()],
    None,
    None,
    1024 * 1024,
  )
  .await?;
  let metadata = process::run(
    tool,
    &["-tvf".into(), source.clone()],
    None,
    None,
    1024 * 1024,
  )
  .await?;
  let mut total = 0u64;
  for (member, relative) in catalogue(
    std::str::from_utf8(&listing)?,
    std::str::from_utf8(&metadata)?,
  )? {
    let destination = output.join(&relative);
    std::fs::create_dir_all(destination.parent().unwrap())?;
    let limit = if relative == Path::new("sources/boot.wim") {
      2 * 1024 * 1024 * 1024
    } else {
      64 * 1024 * 1024
    };
    process::run(
      tool,
      &["-xOf".into(), source.clone(), "--".into(), member],
      None,
      Some(&destination),
      limit,
    )
    .await?;
    let size = destination.metadata()?.len();
    total = total
      .checked_add(size)
      .ok_or_else(|| anyhow::anyhow!("Boot media size overflow"))?;
    ensure!(
      total <= 2 * 1024 * 1024 * 1024,
      "Windows boot media exceeds its bound"
    );
    ensure!(size > 0, "Empty Windows boot member");
  }
  Ok(total)
}
