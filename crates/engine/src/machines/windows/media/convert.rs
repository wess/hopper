//! Convert Microsoft's setup-media ESD into an owned bootable ISO.

use super::super::{
  image,
  setup::{process, Tools},
};
use anyhow::{ensure, Context};
use std::path::Path;

pub fn count(text: &str) -> anyhow::Result<u32> {
  let mut counts = text
    .lines()
    .filter_map(|line| line.strip_prefix("Image Count:"));
  let count = counts
    .next()
    .context("Missing ESD image count")?
    .trim()
    .parse()?;
  ensure!(
    counts.next().is_none() && (4..=32).contains(&count),
    "Unexpected ESD image count"
  );
  Ok(count)
}

fn text(path: &Path) -> anyhow::Result<String> {
  Ok(path.to_str().context("Invalid media path")?.to_owned())
}

async fn run(program: &Path, args: &[String]) -> anyhow::Result<Vec<u8>> {
  process::run(program, args, None, None, 1024 * 1024).await
}

pub async fn build(esd: &Path, output: &Path, tools: &Tools) -> anyhow::Result<()> {
  ensure!(
    esd.is_absolute() && output.is_absolute(),
    "Conversion paths must be absolute"
  );
  let parent = output.parent().context("Missing conversion parent")?;
  super::cache::directory(parent)?;
  ensure!(
    std::fs::symlink_metadata(output).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
    "Conversion output already exists"
  );
  let stage = tempfile::tempdir_in(parent.parent().context("Missing media staging parent")?)?;
  let files = stage.path().join("files");
  std::fs::create_dir(&files)?;
  let esd = text(esd)?;
  let info = run(&tools.wim, &["info".into(), esd.clone()]).await?;
  let count = count(std::str::from_utf8(&info)?)?;
  run(
    &tools.wim,
    &[
      "apply".into(),
      esd.clone(),
      "1".into(),
      text(&files)?,
      "--check".into(),
      "--quiet".into(),
    ],
  )
  .await?;
  let sources = files.join("sources");
  let boot = sources.join("boot.wim");
  let install = sources.join("install.wim");
  for path in [&boot, &install] {
    ensure!(
      std::fs::symlink_metadata(path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
      "Setup-media image unexpectedly contains a destination WIM"
    );
  }
  for (index, bootable) in [(2, false), (3, true)] {
    let mut args = vec![
      "export".into(),
      esd.clone(),
      index.to_string(),
      text(&boot)?,
      "--compress=LZX".into(),
      "--check".into(),
      "--quiet".into(),
    ];
    if bootable {
      args.push("--boot".into());
    }
    run(&tools.wim, &args).await?;
  }
  for index in 4..=count {
    run(
      &tools.wim,
      &[
        "export".into(),
        esd.clone(),
        index.to_string(),
        text(&install)?,
        "--compress=LZMS".into(),
        "--check".into(),
        "--quiet".into(),
      ],
    )
    .await?;
  }
  let xml = run(
    &tools.wim,
    &["info".into(), text(&install)?, "--xml".into()],
  )
  .await?;
  image::professional(&xml)?;
  for file in [
    &boot,
    &install,
    &files.join("efi/microsoft/boot/efisys.bin"),
  ] {
    crate::machines::native::assets::regular(file, 12 * 1024 * 1024 * 1024)?;
  }
  process::run(
    &tools.image,
    &[
      "-eltorito-platform".into(),
      "efi".into(),
      "-b".into(),
      "efi/microsoft/boot/efisys.bin".into(),
      "-no-emul-boot".into(),
      "-udf".into(),
      "-iso-level".into(),
      "3".into(),
      "-V".into(),
      "HOPPER_WINDOWS".into(),
      text(&files)?,
    ],
    None,
    Some(output),
    12 * 1024 * 1024 * 1024,
  )
  .await?;
  Ok(())
}
