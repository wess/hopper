use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use std::{
  any::Any,
  collections::HashMap,
  sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
  },
};

#[derive(Default)]
pub struct State {
  pub values: Mutex<HashMap<String, Vec<u8>>>,
  pub denied: AtomicBool,
  pub write_denied: AtomicBool,
  pub opens: AtomicUsize,
  pub writes: AtomicUsize,
}

pub struct Builder(pub Arc<State>);
struct Memory {
  key: String,
  state: Arc<State>,
}

impl CredentialBuilderApi for Builder {
  fn build(&self, _: Option<&str>, service: &str, user: &str) -> keyring::Result<Box<Credential>> {
    assert_eq!(service, "io.wess.hopper");
    assert!(user.starts_with("machines.windows."));
    self.0.opens.fetch_add(1, Ordering::SeqCst);
    Ok(Box::new(Memory {
      key: user.to_string(),
      state: self.0.clone(),
    }))
  }
  fn as_any(&self) -> &dyn Any {
    self
  }
}

impl Memory {
  fn access(&self) -> keyring::Result<()> {
    if self.state.denied.load(Ordering::SeqCst) {
      return Err(keyring::Error::Invalid(
        "private backend detail".into(),
        "denied".into(),
      ));
    }
    Ok(())
  }
}

impl CredentialApi for Memory {
  fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
    self.access()?;
    if self.state.write_denied.load(Ordering::SeqCst) {
      return Err(keyring::Error::Invalid(
        "private backend detail".into(),
        "write denied".into(),
      ));
    }
    self
      .state
      .values
      .lock()
      .unwrap()
      .insert(self.key.clone(), value.to_vec());
    self.state.writes.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }
  fn get_secret(&self) -> keyring::Result<Vec<u8>> {
    self.access()?;
    self
      .state
      .values
      .lock()
      .unwrap()
      .get(&self.key)
      .cloned()
      .ok_or(keyring::Error::NoEntry)
  }
  fn delete_credential(&self) -> keyring::Result<()> {
    self.access()?;
    self
      .state
      .values
      .lock()
      .unwrap()
      .remove(&self.key)
      .map(|_| ())
      .ok_or(keyring::Error::NoEntry)
  }
  fn as_any(&self) -> &dyn Any {
    self
  }
  fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("Guest credential test backend")
  }
}
