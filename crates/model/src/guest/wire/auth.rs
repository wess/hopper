mod frames;
mod material;
mod session;

use super::require;
use crate::GuestOs;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io;
use zeroize::Zeroize;

pub use material::Material;
pub use session::Session;

pub struct Secret([u8; 32]);

impl Secret {
  pub fn generate() -> io::Result<Self> {
    Self::from_bytes(random()?)
  }

  pub fn from_bytes(bytes: [u8; 32]) -> io::Result<Self> {
    require(bytes != [0; 32])?;
    Ok(Self(bytes))
  }

  pub fn write(&self, writer: &mut impl io::Write) -> io::Result<()> {
    writer.write_all(&self.0)
  }

  pub fn read(reader: &mut impl io::Read) -> io::Result<Self> {
    let mut secret = Self([0; 32]);
    reader.read_exact(&mut secret.0)?;
    require(secret.0 != [0; 32])?;
    Ok(secret)
  }

  fn mac(&self, domain: &[u8], bytes: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).unwrap();
    mac.update(b"hopper.guest.auth.v1\0");
    mac.update(domain);
    mac.update(&[0]);
    mac.update(bytes);
    mac
  }

  fn sign(&self, domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    self.mac(domain, bytes).finalize().into_bytes().into()
  }

  fn verify(&self, domain: &[u8], bytes: &[u8], proof: &[u8]) -> io::Result<()> {
    require(self.mac(domain, bytes).verify_slice(proof).is_ok())
  }
}

impl Drop for Secret {
  fn drop(&mut self) {
    self.0.zeroize();
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Boot {
  pub machine: String,
  pub generation: u64,
  pub guest: GuestOs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
  pub boot: Boot,
  pub policy: u64,
}

impl Scope {
  fn bytes(&self) -> io::Result<[u8; 33]> {
    let id = uuid::Uuid::parse_str(&self.boot.machine).map_err(io::Error::other)?;
    require(!id.is_nil() && id.to_string() == self.boot.machine && self.boot.generation != 0)?;
    let mut bytes = [0; 33];
    bytes[..16].copy_from_slice(id.as_bytes());
    bytes[16..24].copy_from_slice(&self.boot.generation.to_be_bytes());
    bytes[24] = match self.boot.guest {
      GuestOs::Linux => 1,
      GuestOs::Macos => 2,
      GuestOs::Windows => 3,
    };
    bytes[25..].copy_from_slice(&self.policy.to_be_bytes());
    Ok(bytes)
  }

  fn decode(bytes: &[u8]) -> io::Result<Self> {
    require(bytes.len() == 33)?;
    let scope = Self {
      boot: Boot {
        machine: uuid::Uuid::from_slice(&bytes[..16])
          .map_err(io::Error::other)?
          .to_string(),
        generation: u64::from_be_bytes(bytes[16..24].try_into().unwrap()),
        guest: match bytes[24] {
          1 => GuestOs::Linux,
          2 => GuestOs::Macos,
          3 => GuestOs::Windows,
          _ => {
            return Err(io::Error::new(
              io::ErrorKind::InvalidData,
              "Invalid guest OS",
            ))
          }
        },
      },
      policy: u64::from_be_bytes(bytes[25..].try_into().unwrap()),
    };
    scope.bytes()?;
    Ok(scope)
  }
}

pub struct HostHandshake {
  key: Secret,
  scope: Scope,
  nonce: [u8; 32],
}

impl HostHandshake {
  pub fn begin(key: Secret, scope: Scope) -> io::Result<Self> {
    scope.bytes()?;
    Ok(Self {
      key,
      scope,
      nonce: random()?,
    })
  }

  pub fn hello(&self) -> [u8; 101] {
    let mut hello = [0; 101];
    hello[..4].copy_from_slice(b"HGH1");
    hello[4..37].copy_from_slice(&self.scope.bytes().unwrap());
    hello[37..69].copy_from_slice(&self.nonce);
    let proof = self.key.sign(b"hello", &hello[..69]);
    hello[69..].copy_from_slice(&proof);
    hello
  }

  pub fn finish(self, answer: &[u8]) -> io::Result<(Session, [u8; 36])> {
    require(answer.len() == 68 && &answer[..4] == b"HGR1")?;
    let transcript = transcript(&self.scope, &self.nonce, &answer[4..36])?;
    self.key.verify(b"answer", &transcript, &answer[36..])?;
    let mut confirmation = [0; 36];
    confirmation[..4].copy_from_slice(b"HGC1");
    confirmation[4..].copy_from_slice(&self.key.sign(b"confirm", &transcript));
    Ok((
      Session::new(&self.key, self.scope, &transcript, true),
      confirmation,
    ))
  }
}

pub struct GuestHandshake {
  key: Secret,
  scope: Scope,
  transcript: [u8; 97],
}

impl GuestHandshake {
  pub fn answer(key: Secret, boot: &Boot, hello: &[u8]) -> io::Result<(Self, [u8; 68])> {
    require(hello.len() == 101 && &hello[..4] == b"HGH1")?;
    key.verify(b"hello", &hello[..69], &hello[69..])?;
    let scope = Scope::decode(&hello[4..37])?;
    require(&scope.boot == boot)?;
    let nonce = random()?;
    let transcript = transcript(&scope, &hello[37..69], &nonce)?;
    let mut answer = [0; 68];
    answer[..4].copy_from_slice(b"HGR1");
    answer[4..36].copy_from_slice(&nonce);
    answer[36..].copy_from_slice(&key.sign(b"answer", &transcript));
    Ok((
      Self {
        key,
        scope,
        transcript,
      },
      answer,
    ))
  }

  pub fn finish(self, confirmation: &[u8]) -> io::Result<Session> {
    require(confirmation.len() == 36 && &confirmation[..4] == b"HGC1")?;
    self
      .key
      .verify(b"confirm", &self.transcript, &confirmation[4..])?;
    Ok(Session::new(&self.key, self.scope, &self.transcript, false))
  }
}

fn transcript(scope: &Scope, host: &[u8], guest: &[u8]) -> io::Result<[u8; 97]> {
  require(host.len() == 32 && guest.len() == 32)?;
  let mut bytes = [0; 97];
  bytes[..33].copy_from_slice(&scope.bytes()?);
  bytes[33..65].copy_from_slice(host);
  bytes[65..].copy_from_slice(guest);
  Ok(bytes)
}

fn random() -> io::Result<[u8; 32]> {
  let mut bytes = [0; 32];
  getrandom::fill(&mut bytes).map_err(io::Error::other)?;
  require(bytes != [0; 32])?;
  Ok(bytes)
}
