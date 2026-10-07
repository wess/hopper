use anyhow::{ensure, Context};
use serde::Deserialize;

#[derive(Deserialize)]
struct Images {
  #[serde(rename = "IMAGE", default)]
  images: Vec<Image>,
}

#[derive(Deserialize)]
struct Image {
  #[serde(rename = "@INDEX")]
  index: u32,
  #[serde(rename = "WINDOWS")]
  windows: Windows,
}

#[derive(Deserialize)]
struct Windows {
  #[serde(rename = "ARCH")]
  architecture: u32,
  #[serde(rename = "EDITIONID")]
  edition: String,
}

/// Select the edition from WIM metadata rather than assuming a catalogue image order.
pub fn professional(xml: &[u8]) -> anyhow::Result<u32> {
  ensure!(
    !xml.is_empty() && xml.len() <= 1024 * 1024,
    "Invalid Windows image metadata size"
  );
  let text = if xml.starts_with(&[0xff, 0xfe]) || xml.starts_with(&[0xfe, 0xff]) {
    ensure!(
      xml.len().is_multiple_of(2),
      "Truncated Windows image metadata"
    );
    let little = xml[0] == 0xff;
    let words: Vec<_> = xml[2..]
      .as_chunks::<2>()
      .0
      .iter()
      .map(|pair| {
        let bytes = [pair[0], pair[1]];
        if little {
          u16::from_le_bytes(bytes)
        } else {
          u16::from_be_bytes(bytes)
        }
      })
      .collect();
    String::from_utf16(&words)?
  } else {
    std::str::from_utf8(xml)?
      .trim_start_matches('\u{feff}')
      .to_string()
  };
  let images: Images = quick_xml::de::from_str(&text)?;
  ensure!(
    (1..=32).contains(&images.images.len()),
    "Invalid Windows image count"
  );
  let mut indices = std::collections::HashSet::new();
  let mut selected = None;
  for image in images.images {
    ensure!(
      (1..=32).contains(&image.index) && indices.insert(image.index),
      "Invalid or duplicate Windows image index"
    );
    if image.windows.architecture == 12 && image.windows.edition == "Professional" {
      ensure!(
        selected.is_none(),
        "Multiple ARM64 Windows Professional images"
      );
      selected = Some(image.index);
    }
  }
  selected.context("Installation media has no ARM64 Windows Professional image")
}
