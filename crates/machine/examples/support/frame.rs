use anyhow::{ensure, Context};
use machine::devices::virtio::{gpu, pci};
use std::io::Write;

pub fn export(graphics: &mut pci::Device, arguments: &[String]) -> anyhow::Result<()> {
  ensure!(
    pci::fault(graphics).is_none(),
    "Virtio GPU fault: {:?}",
    pci::fault(graphics)
  );
  let linear = pci::read(graphics, gpu::linear::BASE + 28, 4)? == 1;
  let frame = pci::display(graphics)
    .and_then(gpu::frame)
    .context("Firmware produced no GPU frame")?;
  ensure!(
    pci::completed(graphics) > 0,
    "Firmware did not service graphics requests"
  );
  eprintln!(
    "Firmware rendered {}x{} frame in {} GPU requests (linear framebuffer: {linear})",
    frame.width,
    frame.height,
    pci::completed(graphics)
  );
  if let Some(index) = arguments.iter().position(|argument| argument == "--frame") {
    let path = arguments
      .get(index + 1)
      .context("Provide a frame output path")?;
    let file = std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(path)?;
    let mut file = std::io::BufWriter::new(file);
    write!(file, "P6\n{} {}\n255\n", frame.width, frame.height)?;
    for pixel in frame.rgba.as_chunks::<4>().0 {
      file.write_all(&pixel[..3])?;
    }
    file.flush()?;
  }
  Ok(())
}
