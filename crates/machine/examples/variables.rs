#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::runtime::{self, variables, Boot, Control, Reason};
  use std::{
    path::Path,
    time::{Duration, Instant},
  };
  let root = std::env::args()
    .nth(1)
    .context("Provide a new private variable store path")?;
  let root = Path::new(&root);
  let mut store = variables::create(root, &[0xff; 4])?;
  let boot = |code: &[u32], store: &variables::Variables| Boot {
    firmware: code.iter().flat_map(|word| word.to_le_bytes()).collect(),
    variables: variables::bytes(store).to_vec(),
    memory: 0x10000000,
    cpus: 2,
    timeout: Duration::from_secs(3),
  };
  let empty = || std::array::from_fn(|_| None);
  // program one NOR word at 0x04000000, then spin until the owner callback fails.
  let code = [
    0xd2a08001, 0x52800802, 0x72a00802, 0xb9000022, 0x528acf02, 0x72a24682, 0xb9000022, 0x14000000,
  ];
  let mut start = None;
  let result = runtime::run_persistent(boot(&code, &store), empty(), &mut store, |_, _, _| {
    if start.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(200) {
      anyhow::bail!("variable callback check");
    }
    Ok(Control::Continue)
  });
  ensure!(
    result
      .err()
      .is_some_and(|error| error.to_string() == "variable callback check"),
    "Variable probe returned an unexpected result"
  );
  ensure!(
    variables::bytes(&store)[..4] == 0x12345678u32.to_le_bytes(),
    "Guest did not commit its NOR write"
  );
  drop(store);
  let mut store = variables::open(root)?;
  ensure!(
    variables::bytes(&store)[..4] == 0x12345678u32.to_le_bytes(),
    "Variable write was lost after callback failure"
  );
  let code = [0x52800100, 0x72b08000, 0xd4000002];
  let stopped = runtime::run_persistent(boot(&code, &store), empty(), &mut store, |_, _, _| {
    Ok(Control::Continue)
  })?;
  ensure!(
    stopped.reason == Reason::Shutdown && stopped.variables == variables::bytes(&store),
    "Persistent boot did not retain its variable state"
  );
  drop(stopped);
  drop(store);
  let store = variables::open(root)?;
  ensure!(
    variables::bytes(&store)[..4] == 0x12345678u32.to_le_bytes(),
    "Reboot changed the committed NOR word"
  );
  eprintln!(
    "Native persistent NOR write verified across callback failure, reopen and guest shutdown"
  );
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires an Apple silicon Mac")
}
