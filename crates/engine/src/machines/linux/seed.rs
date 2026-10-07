use super::provision::Plan;
use anyhow::ensure;

const SECTOR: usize = 2048;

pub fn image(plan: &Plan) -> anyhow::Result<Vec<u8>> {
  ensure!(
    plan.user_data.starts_with("#cloud-config\n")
      && (1..=65536).contains(&plan.user_data.len())
      && (1..=4096).contains(&plan.meta_data.len())
      && !plan.user_data.contains('\0')
      && !plan.meta_data.contains('\0'),
    "NoCloud configuration exceeds bounds"
  );
  let user = 25;
  let meta = user + plan.user_data.len().div_ceil(SECTOR);
  let sectors = meta + plan.meta_data.len().div_ceil(SECTOR);
  let mut image = vec![0; sectors * SECTOR];
  for (index, joliet, root, little, big) in [(16, false, 23, 19, 20), (17, true, 24, 21, 22)] {
    let descriptor = &mut image[index * SECTOR..(index + 1) * SECTOR];
    descriptor[0] = if joliet { 2 } else { 1 };
    descriptor[1..6].copy_from_slice(b"CD001");
    descriptor[6] = 1;
    if joliet {
      descriptor[8..40].copy_from_slice(&text("HOPPER", 32));
      descriptor[40..72].copy_from_slice(&text("CIDATA", 32));
      descriptor[88..91].copy_from_slice(b"%/E");
    } else {
      descriptor[8..72].fill(b' ');
      descriptor[8..14].copy_from_slice(b"HOPPER");
      descriptor[40..46].copy_from_slice(b"CIDATA");
    }
    both32(&mut descriptor[80..88], sectors as u32);
    both16(&mut descriptor[120..124], 1);
    both16(&mut descriptor[124..128], 1);
    both16(&mut descriptor[128..132], SECTOR as u16);
    both32(&mut descriptor[132..140], 10);
    descriptor[140..144].copy_from_slice(&(little as u32).to_le_bytes());
    descriptor[148..152].copy_from_slice(&(big as u32).to_be_bytes());
    descriptor[156..190].copy_from_slice(&record(&[0], root, SECTOR, true));
    descriptor[881] = 1;
    for (path, big_endian) in [(little, false), (big, true)] {
      let table = &mut image[path * SECTOR..(path + 1) * SECTOR];
      table[0] = 1;
      table[2..6].copy_from_slice(&if big_endian {
        (root as u32).to_be_bytes()
      } else {
        (root as u32).to_le_bytes()
      });
      table[6..8].copy_from_slice(&if big_endian {
        1u16.to_be_bytes()
      } else {
        1u16.to_le_bytes()
      });
    }
    let mut entries = record(&[0], root, SECTOR, true);
    entries.extend(record(&[1], root, SECTOR, true));
    for (name, extent, size) in [
      (
        if joliet { "user-data;1" } else { "USERDATA.;1" },
        user,
        plan.user_data.len(),
      ),
      (
        if joliet { "meta-data;1" } else { "METADATA.;1" },
        meta,
        plan.meta_data.len(),
      ),
    ] {
      let name = if joliet {
        text(name, name.len() * 2)
      } else {
        name.as_bytes().to_vec()
      };
      entries.extend(record(&name, extent, size, false));
    }
    image[root * SECTOR..root * SECTOR + entries.len()].copy_from_slice(&entries);
  }
  image[18 * SECTOR] = 255;
  image[18 * SECTOR + 1..18 * SECTOR + 6].copy_from_slice(b"CD001");
  image[18 * SECTOR + 6] = 1;
  image[user * SECTOR..user * SECTOR + plan.user_data.len()]
    .copy_from_slice(plan.user_data.as_bytes());
  image[meta * SECTOR..meta * SECTOR + plan.meta_data.len()]
    .copy_from_slice(plan.meta_data.as_bytes());
  Ok(image)
}

fn text(value: &str, length: usize) -> Vec<u8> {
  let mut data: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
  while data.len() < length {
    data.extend(32u16.to_be_bytes());
  }
  data
}

fn record(name: &[u8], extent: usize, size: usize, directory: bool) -> Vec<u8> {
  let mut data = vec![0; (33 + name.len()).next_multiple_of(2)];
  data[0] = data.len() as u8;
  both32(&mut data[2..10], extent as u32);
  both32(&mut data[10..18], size as u32);
  data[18..25].copy_from_slice(&[126, 1, 1, 0, 0, 0, 0]);
  data[25] = if directory { 2 } else { 0 };
  both16(&mut data[28..32], 1);
  data[32] = name.len() as u8;
  data[33..33 + name.len()].copy_from_slice(name);
  data
}

fn both32(target: &mut [u8], value: u32) {
  target[..4].copy_from_slice(&value.to_le_bytes());
  target[4..].copy_from_slice(&value.to_be_bytes());
}

fn both16(target: &mut [u8], value: u16) {
  target[..2].copy_from_slice(&value.to_le_bytes());
  target[2..].copy_from_slice(&value.to_be_bytes());
}
