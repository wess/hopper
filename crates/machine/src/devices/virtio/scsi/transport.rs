use super::{command, Optical, MAX_TRANSFER};
use crate::{
  devices::virtio::queue::Chain,
  dma::{self, Memory, Span},
};
use anyhow::ensure;

fn target(lun: &[u8]) -> bool {
  lun == [1, 0, 0, 0, 0, 0, 0, 0] || lun == [1, 0, 0x40, 0, 0, 0, 0, 0]
}

pub fn execute(
  media: &mut Optical,
  memory: &mut impl Memory,
  chain: &Chain,
  index: usize,
) -> anyhow::Result<u32> {
  ensure!(index != 1, "Optical event queue has no negotiated events");
  ensure!(index < 3, "Invalid optical queue");
  media.queues[index] += 1;
  let readable: Vec<Span> = chain
    .buffers
    .iter()
    .filter(|b| !b.writable)
    .map(|b| b.span)
    .collect();
  let writable: Vec<Span> = chain
    .buffers
    .iter()
    .filter(|b| b.writable)
    .map(|b| b.span)
    .collect();
  let mut writes = false;
  for buffer in &chain.buffers {
    ensure!(
      !writes || buffer.writable,
      "SCSI readable buffer follows its response"
    );
    writes |= buffer.writable;
  }
  if index == 0 {
    let mut kind = [0; 4];
    dma::read(memory, &readable, 0, &mut kind)?;
    let kind = u32::from_le_bytes(kind);
    if matches!(kind, 1 | 2) {
      ensure!(
        dma::length(&readable) >= 16 && dma::length(&writable) >= 5,
        "Truncated SCSI notification query"
      );
      let mut request = [0; 16];
      dma::read(memory, &readable, 0, &mut request)?;
      let response = if target(&request[4..12]) { 0 } else { 3 };
      dma::write(memory, &writable, 0, &[0, 0, 0, 0, response])?;
      return Ok(5);
    }
    ensure!(
      dma::length(&readable) >= 24 && dma::length(&writable) >= 1,
      "Truncated SCSI control request"
    );
    let mut request = [0; 24];
    dma::read(memory, &readable, 0, &mut request)?;
    let kind = u32::from_le_bytes(request[..4].try_into()?);
    let subtype = u32::from_le_bytes(request[4..8].try_into()?);
    media.control = Some((kind, subtype, request[8..16].try_into()?));
    let response = if kind != 0 {
      9
    } else if subtype > 7 {
      11
    } else if !target(&request[8..16]) {
      3
    } else {
      0
    };
    dma::write(memory, &writable, 0, &[response])?;
    return Ok(1);
  }
  ensure!(index == 2, "Invalid optical request queue");
  let request_len = 19 + media.cdb_size as usize;
  let response_len = 12 + media.sense_size as usize;
  ensure!(
    dma::length(&readable) >= request_len as u64 && dma::length(&writable) >= response_len as u64,
    "Truncated SCSI request or response"
  );
  let outgoing = dma::length(&readable) - request_len as u64;
  let incoming = dma::length(&writable) - response_len as u64;
  ensure!(
    outgoing <= MAX_TRANSFER as u64 && incoming <= MAX_TRANSFER as u64,
    "SCSI DMA exceeds transfer limit"
  );
  let mut request = vec![0; request_len];
  dma::read(memory, &readable, 0, &mut request)?;
  let mut response = vec![0; response_len];
  let mut data = Vec::new();
  if !target(&request[..8]) && !(request[..8] == [0xc1, 1, 0, 0, 0, 0, 0, 0] && request[19] == 0xa0)
  {
    response[11] = 3;
    media.missing = Some(request[..8].try_into()?);
  } else if outgoing != 0 && incoming != 0 {
    media.transfer = Some((outgoing, incoming));
    response[11] = 9;
  } else {
    let reply = command::execute(media, &request[19..])?;
    if let Some(sense) = reply.sense {
      let count = sense.len().min(media.sense_size as usize);
      response[..4].copy_from_slice(&(count as u32).to_le_bytes());
      response[10] = 2;
      response[12..12 + count].copy_from_slice(&sense[..count]);
    } else if reply.data.len() as u64 > incoming {
      response[11] = 1;
    } else {
      data = reply.data;
    }
  }
  response[4..8].copy_from_slice(&((incoming + outgoing - data.len() as u64) as u32).to_le_bytes());
  dma::write(memory, &writable, 0, &response)?;
  dma::write(memory, &writable, response_len as u64, &data)?;
  Ok((response_len + data.len()) as u32)
}
