use super::{storage, State, SECTOR, SIZE};
use anyhow::ensure;
use sha2::{Digest, Sha256};

pub(super) struct Metadata {
  pub sense_size: u32,
  pub cdb_size: u32,
  pub sense: [u8; 18],
  pub inserted: bool,
  pub commands: Box<[u64; 256]>,
  pub errors: u64,
  pub queues: [u64; 3],
  pub missing: Option<[u8; 8]>,
  pub control: Option<(u32, u32, [u8; 8])>,
  pub transfer: Option<(u64, u64)>,
}

pub(super) fn encode(saved: &State, image: &[u8; 76]) -> anyhow::Result<[u8; SIZE]> {
  let mut bytes = [0; SIZE];
  bytes[..4].copy_from_slice(b"HDO1");
  bytes[4..12].copy_from_slice(&saved.sectors.to_le_bytes());
  bytes[12..16].copy_from_slice(&saved.sense_size.to_le_bytes());
  bytes[16..20].copy_from_slice(&saved.cdb_size.to_le_bytes());
  bytes[20..38].copy_from_slice(&saved.sense);
  bytes[38] = saved.inserted as u8;
  for (chunk, value) in bytes[39..2087]
    .as_chunks_mut::<8>()
    .0
    .iter_mut()
    .zip(saved.commands.iter())
  {
    *chunk = value.to_le_bytes();
  }
  bytes[2087..2095].copy_from_slice(&saved.errors.to_le_bytes());
  for (chunk, value) in bytes[2095..2119]
    .as_chunks_mut::<8>()
    .0
    .iter_mut()
    .zip(saved.queues)
  {
    *chunk = value.to_le_bytes();
  }
  if let Some(lun) = saved.missing {
    bytes[2119] = 1;
    bytes[2120..2128].copy_from_slice(&lun);
  }
  if let Some((kind, subtype, lun)) = saved.control {
    bytes[2128] = 1;
    bytes[2129..2133].copy_from_slice(&kind.to_le_bytes());
    bytes[2133..2137].copy_from_slice(&subtype.to_le_bytes());
    bytes[2137..2145].copy_from_slice(&lun);
  }
  if let Some((outgoing, incoming)) = saved.transfer {
    bytes[2145] = 1;
    bytes[2146..2154].copy_from_slice(&outgoing.to_le_bytes());
    bytes[2154..2162].copy_from_slice(&incoming.to_le_bytes());
  }
  bytes[2162..2238].copy_from_slice(image);
  let checksum = Sha256::digest(&bytes[..2238]);
  bytes[2238..].copy_from_slice(&checksum);
  decode(&bytes, saved.sectors)?;
  Ok(bytes)
}

pub(super) fn decode(bytes: &[u8], expected: u64) -> anyhow::Result<Metadata> {
  ensure!(
    bytes.len() == SIZE && &bytes[..4] == b"HDO1",
    "Unknown or malformed optical record"
  );
  ensure!(
    Sha256::digest(&bytes[..2238])[..] == bytes[2238..],
    "Optical record checksum mismatch"
  );
  let sectors = u64::from_le_bytes(bytes[4..12].try_into()?);
  ensure!(
    sectors == expected,
    "Optical record capacity binding mismatch"
  );
  let size = sectors
    .checked_mul(SECTOR)
    .ok_or_else(|| anyhow::anyhow!("Optical capacity overflow"))?;
  storage::record::validate(&bytes[2162..2238], size)?;
  let sense_size = u32::from_le_bytes(bytes[12..16].try_into()?);
  let cdb_size = u32::from_le_bytes(bytes[16..20].try_into()?);
  ensure!(
    sense_size <= 256 && (6..=256).contains(&cdb_size),
    "Optical negotiation is invalid"
  );
  ensure!(
    [38, 2119, 2128, 2145]
      .iter()
      .all(|&offset| bytes[offset] <= 1),
    "Optical record boolean is invalid"
  );
  let sense: [u8; 18] = bytes[20..38].try_into()?;
  let mut canonical = [0; 18];
  if sense != canonical {
    canonical[0] = 0x70;
    canonical[2] = sense[2];
    canonical[7] = 10;
    canonical[12] = sense[12];
    ensure!(
      matches!(
        (sense[2], sense[12]),
        (2, 0x3a) | (5, 0x20 | 0x21 | 0x24) | (7, 0x27)
      ) && sense == canonical,
      "Optical pending sense is invalid"
    );
  }
  let mut commands = Box::new([0; 256]);
  for (value, chunk) in commands.iter_mut().zip(bytes[39..2087].as_chunks::<8>().0) {
    *value = u64::from_le_bytes(*chunk);
  }
  let errors = u64::from_le_bytes(bytes[2087..2095].try_into()?);
  let mut queues = [0; 3];
  for (value, chunk) in queues.iter_mut().zip(bytes[2095..2119].as_chunks::<8>().0) {
    *value = u64::from_le_bytes(*chunk);
  }
  ensure!(
    queues[1] == 0,
    "Optical event queue has no negotiated events"
  );
  let missing = if bytes[2119] == 1 {
    Some(bytes[2120..2128].try_into()?)
  } else {
    ensure!(
      bytes[2120..2128] == [0; 8],
      "Unused optical missing LUN is nonzero"
    );
    None
  };
  let control = if bytes[2128] == 1 {
    let kind = u32::from_le_bytes(bytes[2129..2133].try_into()?);
    ensure!(
      !matches!(kind, 1 | 2),
      "Optical notification cannot be a retained control"
    );
    Some((
      kind,
      u32::from_le_bytes(bytes[2133..2137].try_into()?),
      bytes[2137..2145].try_into()?,
    ))
  } else {
    ensure!(
      bytes[2129..2145] == [0; 16],
      "Unused optical control is nonzero"
    );
    None
  };
  let transfer = if bytes[2145] == 1 {
    let outgoing = u64::from_le_bytes(bytes[2146..2154].try_into()?);
    let incoming = u64::from_le_bytes(bytes[2154..2162].try_into()?);
    let limit = super::super::super::MAX_TRANSFER as u64;
    ensure!(
      (1..=limit).contains(&outgoing) && (1..=limit).contains(&incoming),
      "Optical retained transfer exceeds bounds"
    );
    Some((outgoing, incoming))
  } else {
    ensure!(
      bytes[2146..2162] == [0; 16],
      "Unused optical transfer is nonzero"
    );
    None
  };
  Ok(Metadata {
    sense_size,
    cdb_size,
    sense,
    inserted: bytes[38] == 1,
    commands,
    errors,
    queues,
    missing,
    control,
    transfer,
  })
}
