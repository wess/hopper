use super::{
  command::{self, Reply},
  Optical,
};

pub(super) fn execute(media: &mut Optical, cdb: &[u8]) -> Reply {
  let allocation = if cdb[0] == 0x1a {
    cdb[4] as usize
  } else {
    u16::from_be_bytes([cdb[7], cdb[8]]) as usize
  };
  let mut data = match cdb[0] {
    0x1a | 0x5a => {
      if !matches!(cdb[2] & 0x3f, 0x2a | 0x3f) || cdb[2] >> 6 == 3 || cdb[3] != 0 {
        return command::error(media, 5, 0x24);
      }
      let header = if cdb[0] == 0x1a { 4 } else { 8 };
      let mut bytes = vec![0; header + 20];
      if header == 4 {
        bytes[0] = (bytes.len() - 1) as u8;
        bytes[2] = 0x80;
      } else {
        let length = (bytes.len() as u16 - 2).to_be_bytes();
        bytes[..2].copy_from_slice(&length);
        bytes[3] = 0x80;
      }
      bytes[header] = 0x2a;
      bytes[header + 1] = 18;
      // only DVD-ROM reading; no recordable media or write capabilities.
      if cdb[2] >> 6 != 1 {
        bytes[header + 2] = 8;
      }
      bytes
    }
    0x43 => {
      let format = cdb[2] & 15;
      if format > 1 || !matches!(cdb[6], 0 | 1 | 0xaa) {
        return command::error(media, 5, 0x24);
      }
      let tracks: Vec<(u8, u64)> = if format == 1 {
        vec![(1, 0)]
      } else if cdb[6] == 0xaa {
        vec![(0xaa, media.sectors)]
      } else {
        vec![(1, 0), (0xaa, media.sectors)]
      };
      let mut bytes = vec![0, (2 + tracks.len() * 8) as u8, 1, 1];
      for (track, sector) in tracks {
        let address = if cdb[1] & 2 != 0 {
          let frames = sector + 150;
          if frames / 4500 > 255 {
            return command::error(media, 5, 0x24);
          }
          [
            0,
            (frames / 4500) as u8,
            (frames / 75 % 60) as u8,
            (frames % 75) as u8,
          ]
        } else {
          if sector > u32::MAX as u64 {
            return command::error(media, 5, 0x24);
          }
          (sector as u32).to_be_bytes()
        };
        bytes.extend([0, 0x14, track, 0]);
        bytes.extend(address);
      }
      bytes
    }
    0x46 => {
      if cdb[1] & 3 == 3 {
        return command::error(media, 5, 0x24);
      }
      let start = u16::from_be_bytes([cdb[2], cdb[3]]);
      let mut bytes = vec![0; 8];
      bytes[7] = if media.inserted { 0x10 } else { 0 };
      let active = u8::from(media.inserted);
      for (code, flags, data) in [
        (0u16, 3, vec![0, 0x10, active, 0]),
        (1, 3, vec![0; 8]),
        (2, 3, vec![0; 4]),
        (3, 3, vec![0x18, 0, 0, 0]),
        (0x10, active, vec![0, 0, 8, 0, 0, 1, 0, 0]),
        (0x1f, active, vec![0; 4]),
      ] {
        let kind = cdb[1] & 3;
        if code < start || (kind == 2 && code != start) || (kind == 1 && flags & 1 == 0) {
          continue;
        }
        bytes.extend(code.to_be_bytes());
        bytes.extend([flags, data.len() as u8]);
        bytes.extend(data);
      }
      let length = (bytes.len() as u32 - 4).to_be_bytes();
      bytes[..4].copy_from_slice(&length);
      bytes
    }
    0x4a => {
      if cdb[1] & 1 == 0 {
        return command::error(media, 5, 0x24);
      }
      if cdb[4] & 0x10 == 0 {
        vec![0, 2, 0x80, 0x10]
      } else {
        vec![0, 6, 4, 0x10, 0, if media.inserted { 2 } else { 0 }, 0, 0]
      }
    }
    _ => return command::error(media, 5, 0x20),
  };
  data.truncate(if cdb[0] == 0x1a {
    cdb[4] as usize
  } else {
    allocation
  });
  Reply { data, sense: None }
}
