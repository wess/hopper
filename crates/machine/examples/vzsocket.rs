#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{self, Action, Boot, Linux};
  use objc2::MainThreadMarker;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    os::unix::fs::OpenOptionsExt,
    sync::{
      atomic::{AtomicU64, Ordering},
      Arc,
    },
    time::{Duration, Instant},
  };

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 3,
    "Provide verified kernel, socket initramfs and matching modloop"
  );
  let main = MainThreadMarker::new().context("Socket probe requires the main thread")?;
  let root = tempfile::tempdir()?;
  let disk = root.path().join("disk");
  std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&disk)?
    .set_len(1 << 30)?;
  let output = root.path().join("console");
  let console = std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&output)?;
  let boot = Linux {
    cpus: 2,
    memory: 512 << 20,
    width: 1024,
    height: 768,
    identity: vz::identity(),
    boot: Boot::Kernel {
      kernel: args[0].clone().into(),
      initramfs: Some(args[1].clone().into()),
      command_line: "console=hvc0 rdinit=/init".into(),
    },
    disk,
    installer: Some(args[2].clone().into()),
    seed: None,
    network: None,
    console: Some(console),
    shares: Vec::new(),
    speakers: false,
  };
  let mut vm = vz::create(main, &boot)?;
  let ownership = Arc::new(());
  let weak = Arc::downgrade(&ownership);
  vz::retain(&mut vm, ownership.clone())?;
  drop(ownership);
  let generation = Arc::new(AtomicU64::new(0));
  let policy = generation.clone();
  let check: vz::queue::Check = Arc::new(move || {
    ensure!(
      policy.load(Ordering::SeqCst) == 0,
      "Original policy was revoked"
    );
    Ok(())
  });
  let socket = vz::socket::listen(&vm, check.clone())?;
  ensure!(
    vz::socket::listen(&vm, check).is_err(),
    "Duplicate listener was admitted"
  );
  let run_loop = NSRunLoop::currentRunLoop();
  let tick = || {
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  };
  let wait = |pending: vz::Pending| -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
      if let Some(result) = vz::poll(&pending)? {
        return result;
      }
      ensure!(Instant::now() < deadline, "VZ transition timed out");
      tick();
    }
  };
  wait(vz::transition(&vm, Action::Start)?)?;
  let result = (|| -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut payload = Vec::new();
    let mut acknowledged = 0;
    loop {
      ensure!(Instant::now() < deadline, "Guest socket timed out");
      ensure!(
        std::fs::metadata(&output)?.len() <= 1 << 20,
        "Serial output exceeds bounds"
      );
      let text = std::fs::read_to_string(&output)?;
      ensure!(
        !text.contains("HOPPER_SOCKET_FAILED"),
        "Guest socket failed: {text}"
      );
      if text.contains("HOPPER_SOCKET_GUEST_OK") {
        break;
      }
      if socket.connected()? {
        ensure!(
          socket.read(&mut vec![0; (64 << 10) + 1]).is_err(),
          "Oversized read was accepted"
        );
        ensure!(
          socket.write(&vec![0; (64 << 10) + 1]).is_err(),
          "Oversized write was accepted"
        );
        if payload.len() < 11 {
          let mut bytes = [0; 11];
          if let Some(count) = socket.read(&mut bytes[..11 - payload.len()])? {
            ensure!(count > 0, "Guest closed before its greeting");
            payload.extend_from_slice(&bytes[..count]);
          }
        }
        if payload.len() == 11 && acknowledged < 3 {
          ensure!(payload == b"guest-1001\n", "Unexpected guest payload");
          if let Some(count) = socket.write(&b"ok\n"[acknowledged..])? {
            acknowledged += count;
          }
        }
      }
      tick();
    }
    wait(vz::transition(&vm, Action::Pause)?)?;
    ensure!(
      socket.connected().is_err(),
      "Paused hardware kept its guest connection"
    );
    wait(vz::transition(&vm, Action::Resume)?)?;
    ensure!(!socket.connected()?, "Resume reused the paused connection");
    generation.store(1, Ordering::SeqCst);
    ensure!(
      socket.connected().is_err(),
      "Revoked policy kept its guest connection"
    );
    generation.store(2, Ordering::SeqCst);
    ensure!(
      socket.write(b"denied").is_err(),
      "Off/on policy reused an old connection"
    );
    Ok(())
  })();
  let stopped = wait(vz::transition(&vm, Action::Stop)?);
  result?;
  stopped?;
  drop(vm);
  ensure!(
    weak.upgrade().is_some(),
    "Listener released runtime ownership early"
  );
  drop(socket);
  ensure!(
    weak.upgrade().is_none(),
    "Listener leaked runtime ownership"
  );
  let (_client, mut owner) = vz::queue::channel();
  owner.insert("socketprobe", vz::create(main, &boot)?)?;
  let socket = owner.guest_socket("socketprobe", Arc::new(|| Ok(())))?;
  ensure!(
    owner.retire("socketprobe").is_err(),
    "Attached transport allowed retirement"
  );
  drop(socket);
  owner.retire("socketprobe")?;
  println!("Guest-initiated socket exchange as UID 1001, duplicate rejection, bounded I/O, original-policy revocation and runtime ownership verified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ socket probe requires Apple silicon macOS")
}
