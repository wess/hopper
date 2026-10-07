use anyhow::{ensure, Context};
use block2::RcBlock;
use objc2_foundation::NSError;
use objc2_virtualization::VZMacOSRestoreImage;
use serde::Serialize;
use std::sync::mpsc;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
  pub url: String,
  pub build: String,
  pub version: [isize; 3],
  pub hardware: Vec<u8>,
  pub minimum_cpus: usize,
  pub minimum_memory: u64,
}

pub struct Discovery(mpsc::Receiver<anyhow::Result<Image>>);

pub fn latest() -> Discovery {
  let (send, receive) = mpsc::sync_channel(1);
  let completion = RcBlock::new(
    move |image: *mut VZMacOSRestoreImage, error: *mut NSError| {
      let result = if !error.is_null() {
        let message: String = unsafe { &*error }.to_string().chars().take(512).collect();
        Err(anyhow::anyhow!("macOS restore discovery failed: {message}"))
      } else {
        unsafe { image.as_ref() }
          .context("No supported macOS restore image returned")
          .and_then(metadata)
      };
      let _ = send.try_send(result);
    },
  );
  unsafe {
    VZMacOSRestoreImage::fetchLatestSupportedWithCompletionHandler(&completion);
  }
  Discovery(receive)
}

pub fn poll(discovery: &Discovery) -> anyhow::Result<Option<anyhow::Result<Image>>> {
  match discovery.0.try_recv() {
    Ok(result) => Ok(Some(result)),
    Err(mpsc::TryRecvError::Empty) => Ok(None),
    Err(error) => Err(error).context("macOS restore discovery ended without a result"),
  }
}

fn metadata(image: &VZMacOSRestoreImage) -> anyhow::Result<Image> {
  unsafe {
    let requirement = image
      .mostFeaturefulSupportedConfiguration()
      .context("No macOS hardware model is supported on this host")?;
    let model = requirement.hardwareModel();
    ensure!(
      model.isSupported(),
      "macOS restore hardware model is unsupported"
    );
    let url = image.URL();
    ensure!(
      url.user().is_none() && url.password().is_none(),
      "macOS restore URL cannot contain credentials"
    );
    ensure!(
      url
        .scheme()
        .is_some_and(|scheme| scheme.to_string() == "https"),
      "macOS restore URL requires HTTPS"
    );
    ensure!(
      url.host().is_some_and(|host| matches!(
        host.to_string().as_str(),
        "updates.cdn-apple.com" | "updates-http.cdn-apple.com"
      )),
      "macOS restore URL is outside the official delivery origins"
    );
    let version = image.operatingSystemVersion();
    let metadata = Image {
      url: url
        .absoluteString()
        .context("macOS restore URL is missing")?
        .to_string(),
      build: image.buildVersion().to_string(),
      version: [
        version.majorVersion,
        version.minorVersion,
        version.patchVersion,
      ],
      hardware: model.dataRepresentation().to_vec(),
      minimum_cpus: requirement.minimumSupportedCPUCount(),
      minimum_memory: requirement.minimumSupportedMemorySize(),
    };
    ensure!(
      metadata.url.len() <= 4096 && metadata.build.len() <= 128 && metadata.hardware.len() <= 65536,
      "macOS restore metadata exceeds bounds"
    );
    Ok(metadata)
  }
}
