#[cfg(unix)]
use base64::{engine::general_purpose::STANDARD, Engine};
#[cfg(unix)]
use image::ImageEncoder;
#[cfg(unix)]
use std::io::Write;

#[cfg(unix)]
pub(super) async fn native(host: &host::Host, id: &str) -> anyhow::Result<serde_json::Value> {
  let frame = host.capture_native_machine(id).await?;
  let mut output = Output(Vec::new());
  image::codecs::png::PngEncoder::new(&mut output).write_image(
    &frame.rgba,
    frame.width,
    frame.height,
    image::ExtendedColorType::Rgba8,
  )?;
  host.machines().machine(id, host::MachineActor::Agent)?;
  Ok(
    serde_json::json!({"content":[{"type":"image","mimeType":"image/png","data":STANDARD.encode(output.0)}]}),
  )
}

#[cfg(unix)]
struct Output(Vec<u8>);

#[cfg(unix)]
impl Write for Output {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    if bytes.len() > (16 * 1024 * 1024usize).saturating_sub(self.0.len()) {
      return Err(std::io::Error::other("VM screenshot exceeds 16 MiB"));
    }
    self.0.extend_from_slice(bytes);
    Ok(bytes.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}
