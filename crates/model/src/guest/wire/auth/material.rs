use super::{require, Boot, Scope, Secret};
use std::io;

pub struct Material {
  pub boot: Boot,
  pub key: Secret,
}

impl Material {
  pub const LENGTH: usize = 61;
  pub const REQUEST: &[u8; 4] = b"HBR1";

  pub fn handshake(&self, policy: u64) -> io::Result<super::HostHandshake> {
    super::HostHandshake::begin(
      Secret(self.key.0),
      Scope {
        boot: self.boot.clone(),
        policy,
      },
    )
  }

  pub fn write(&self, writer: &mut impl io::Write) -> io::Result<()> {
    let scope = Scope {
      boot: self.boot.clone(),
      policy: 0,
    }
    .bytes()?;
    writer.write_all(b"HGB1")?;
    writer.write_all(&scope[..25])?;
    self.key.write(writer)
  }

  pub fn read(reader: &mut impl io::Read) -> io::Result<Self> {
    let mut header = [0; 29];
    reader.read_exact(&mut header)?;
    require(&header[..4] == b"HGB1")?;
    let mut scope = [0; 33];
    scope[..25].copy_from_slice(&header[4..]);
    let boot = Scope::decode(&scope)?.boot;
    let key = Secret::read(reader)?;
    let mut extra = [0; 1];
    require(reader.read(&mut extra)? == 0)?;
    Ok(Self { boot, key })
  }
}
