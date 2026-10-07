//! Per-VM credentials, isolated from container registry and host account secrets.

use serde::{Deserialize, Serialize};
use std::io::{Error, ErrorKind, Result};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Credentials {
  pub user_password: String,
  pub administrator_password: String,
}

pub struct Slot {
  entry: keyring::Entry,
}

pub fn key(id: &str) -> Result<String> {
  if id.len() != 36 {
    return Err(Error::new(ErrorKind::InvalidInput, "Invalid VM id"));
  }
  let id =
    uuid::Uuid::parse_str(id).map_err(|_| Error::new(ErrorKind::InvalidInput, "Invalid VM id"))?;
  Ok(format!("machines.windows.{id}.accounts"))
}

pub fn open(id: &str) -> Result<Slot> {
  let key = key(id)?;
  let entry = keyring::Entry::new("io.wess.hopper", &key)
    .map_err(|_| Error::other("Guest credential store is unavailable"))?;
  Ok(Slot { entry })
}

fn validate(credentials: &Credentials) -> Result<()> {
  let valid = [
    &credentials.user_password,
    &credentials.administrator_password,
  ]
  .into_iter()
  .all(|value| (8..=128).contains(&value.len()) && !value.chars().any(char::is_control));
  if !valid || credentials.user_password == credentials.administrator_password {
    return Err(Error::new(
      ErrorKind::InvalidData,
      "Guest credentials are invalid",
    ));
  }
  Ok(())
}

pub fn read(slot: &Slot) -> Result<Option<Credentials>> {
  let raw = match slot.entry.get_password() {
    Ok(raw) => raw,
    Err(keyring::Error::NoEntry) => return Ok(None),
    Err(_) => return Err(Error::other("Guest credential store is unavailable")),
  };
  if raw.len() > 4096 {
    return Err(Error::new(
      ErrorKind::InvalidData,
      "Guest credentials are invalid",
    ));
  }
  let credentials = serde_json::from_str(&raw)
    .map_err(|_| Error::new(ErrorKind::InvalidData, "Guest credentials are invalid"))?;
  validate(&credentials)?;
  Ok(Some(credentials))
}

/// caller must hold the VM lock; existing credentials are never replaced during setup.
pub fn create(slot: &Slot, credentials: &Credentials) -> Result<()> {
  validate(credentials)?;
  if read(slot)?.is_some() {
    return Err(Error::new(
      ErrorKind::AlreadyExists,
      "Guest credentials already exist",
    ));
  }
  let raw = serde_json::to_string(credentials)
    .map_err(|_| Error::other("Cannot encode guest credentials"))?;
  slot
    .entry
    .set_password(&raw)
    .map_err(|_| Error::other("Cannot save guest credentials"))
}
