#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    linux,
    vz::{self, Service, Stage},
    Actor, Machines,
  };
  use reqwest::header;
  use std::time::Duration;

  let client = reqwest::Client::builder()
    .redirect(reqwest::redirect::Policy::none())
    .connect_timeout(Duration::from_secs(30))
    .read_timeout(Duration::from_secs(60))
    .build()?;
  let mut sums = client
    .get("https://cdimage.ubuntu.com/ubuntu/releases/24.04/release/SHA256SUMS")
    .timeout(Duration::from_secs(60))
    .send()
    .await?
    .error_for_status()?;
  let mut text = Vec::new();
  while let Some(chunk) = sums.chunk().await? {
    ensure!(
      text.len() + chunk.len() <= 65536,
      "Checksum metadata exceeds its bound"
    );
    text.extend_from_slice(&chunk);
  }
  ensure!(
    std::str::from_utf8(&text)?
      .lines()
      .any(|line| line == format!("{} *ubuntu-24.04.5-desktop-arm64.iso", linux::SHA256)),
    "Official checksum no longer matches the pinned image"
  );
  let head = client.head(linux::URL).send().await?.error_for_status()?;
  ensure!(
    head
      .headers()
      .get(header::CONTENT_LENGTH)
      .context("Missing official length")?
      .to_str()?
      .parse::<u64>()?
      == linux::SIZE,
    "Official size differs from pinned image"
  );
  let mut response = client
    .get(linux::URL)
    .header(header::ACCEPT_ENCODING, "identity")
    .header(header::RANGE, "bytes=0-65535")
    .send()
    .await?
    .error_for_status()?;
  ensure!(
    response.status() == reqwest::StatusCode::PARTIAL_CONTENT
      && response
        .headers()
        .get(header::CONTENT_RANGE)
        .context("Missing official range")?
        .to_str()?
        == format!("bytes 0-65535/{}", linux::SIZE),
    "Official range response differs from requested bytes"
  );
  let mut prefix = Vec::new();
  while let Some(chunk) = response.chunk().await? {
    ensure!(
      prefix.len() + chunk.len() <= 65536,
      "Official range exceeds bounds"
    );
    prefix.extend_from_slice(&chunk);
  }
  ensure!(
    prefix.len() == 65536 && &prefix[32769..32774] == b"CD001",
    "Official range lacks ISO descriptor"
  );
  let root = tempfile::tempdir()?;
  if fs2::available_space(root.path())? < linux::SIZE + (64 << 20) {
    let manager = Machines {
      root: root.path().join("machines"),
    };
    let machine = model::Machine {
      id: "00000000-0000-0000-0000-000000000001".into(),
      name: "Media diagnostic".into(),
      guest: model::GuestOs::Linux,
      profile: "ubuntu".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      agent_access: true,
      agent_generation: 0,
      installer: None,
    };
    store::json::write(
      &manager
        .root
        .join("records")
        .join(format!("{}.json", machine.id)),
      &machine,
    )?;
    let (client, _owner) = vz::channel();
    let service = Service::new(manager.clone(), client);
    let result = service
      .prepare_linux(&machine.id, Actor::Agent, Stage::Installer)
      .await;
    let error = result
      .err()
      .context("Automatic download should reject insufficient storage")?;
    ensure!(
      error.to_string().contains("free disk space"),
      "Unexpected automatic preparation failure: {error}"
    );
    ensure!(
      !manager.root.join("vz").exists(),
      "Low-space preparation published VM state"
    );
    ensure!(
      std::fs::metadata(
        manager
          .root
          .join("native/images/linux")
          .join(format!("{}.part", linux::SHA256))
      )?
      .len()
        == 0,
      "Low-space download wrote partial media"
    );
    println!("Automatic Linux acquisition rejected insufficient space before downloading or publishing VM state");
  }
  println!("Official Ubuntu checksum metadata, pinned size and bounded ISO range verified; no full ISO download or installation performed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native Linux media probe requires Apple silicon macOS")
}
