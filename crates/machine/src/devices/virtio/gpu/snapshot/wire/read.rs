use super::{linear, resource, stream::Reader, Display, Frame, State, LIMIT};
use crate::dma::Span;
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, sync::Arc};

pub fn decode(
  input: &mut impl Read,
  expected: (u32, u32),
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<State> {
  let mut reader = Reader {
    input,
    authorize,
    digest: Sha256::new(),
  };
  let mut magic = [0; 4];
  reader.data(&mut magic)?;
  ensure!(&magic == b"HGF1", "Unknown GPU record version");
  let preferred = (reader.word()?, reader.word()?);
  ensure!(
    preferred == expected && resource::size(preferred.0, preferred.1).is_some(),
    "GPU record display binding mismatch"
  );
  let generation = reader.number()?;
  let count = reader.word()?;
  ensure!(count <= 64, "GPU record resource count exceeds limit");
  let mut resources = BTreeMap::new();
  let mut allocated = 0;
  let mut previous = 0;
  for _ in 0..count {
    let id = reader.word()?;
    let width = reader.word()?;
    let height = reader.word()?;
    let format = reader.word()?;
    let size = resource::size(width, height).context("GPU resource dimensions are invalid")?;
    ensure!(
      id > previous && matches!(format, 1..=4 | 67 | 68 | 121 | 134) && size <= LIMIT - allocated,
      "GPU record resource is invalid"
    );
    previous = id;
    allocated += size;
    let pixels = Arc::new(reader.bytes(size)?);
    let count = reader.word()? as usize;
    ensure!(count <= 4096, "GPU backing count exceeds limit");
    let bytes = reader.bytes(count * 12)?;
    let mut backing = Vec::new();
    backing.try_reserve_exact(count)?;
    for bytes in bytes.as_chunks::<12>().0 {
      let address = u64::from_le_bytes(bytes[..8].try_into()?);
      let length = u32::from_le_bytes(bytes[8..].try_into()?);
      ensure!(
        length != 0 && address.checked_add(u64::from(length)).is_some(),
        "GPU backing range is invalid"
      );
      backing.push(Span { address, length });
    }
    resources.insert(
      id,
      resource::Resource {
        width,
        height,
        format,
        pixels,
        backing: Arc::new(backing),
      },
    );
  }
  let scanout = if reader.boolean()? {
    Some((
      reader.word()?,
      resource::Rect {
        x: reader.word()?,
        y: reader.word()?,
        width: reader.word()?,
        height: reader.word()?,
      },
    ))
  } else {
    None
  };
  let frame = read_frame(&mut reader)?;
  let cursor = super::super::super::cursor::Cursor {
    x: reader.word()?,
    y: reader.word()?,
    hot_x: reader.word()?,
    hot_y: reader.word()?,
    generation: reader.number()?,
    rgba: if reader.boolean()? {
      Some(reader.bytes(16384)?)
    } else {
      None
    },
  };
  let mut registers = [0; 6];
  for register in &mut registers {
    *register = reader.word()?;
  }
  let layout = if reader.boolean()? {
    Some(linear::Layout {
      address: reader.number()?,
      width: reader.word()?,
      height: reader.word()?,
      stride: reader.word()?,
    })
  } else {
    None
  };
  let linear = linear::Linear {
    registers,
    layout,
    frame: read_frame(&mut reader)?,
  };
  (reader.authorize)()?;
  let digest = reader.digest.finalize();
  let mut footer = [0; 32];
  reader.input.read_exact(&mut footer)?;
  ensure!(digest[..] == footer, "GPU record checksum mismatch");
  let mut extra = [0];
  ensure!(
    reader.input.read(&mut extra)? == 0,
    "GPU record has trailing bytes"
  );
  (reader.authorize)()?;
  let display = Display {
    resources,
    allocated,
    preferred,
    scanout,
    frame,
    generation,
    cursor,
    linear,
  };
  super::validate(&display)?;
  Ok(State(display))
}

fn read_frame<R: Read, A: FnMut() -> anyhow::Result<()>>(
  reader: &mut Reader<'_, R, A>,
) -> anyhow::Result<Option<Arc<Frame>>> {
  if !reader.boolean()? {
    return Ok(None);
  }
  let width = reader.word()?;
  let height = reader.word()?;
  let size = resource::size(width, height).context("GPU frame dimensions are invalid")?;
  let generation = reader.number()?;
  Ok(Some(Arc::new(Frame {
    width,
    height,
    generation,
    rgba: reader.bytes(size)?,
  })))
}
