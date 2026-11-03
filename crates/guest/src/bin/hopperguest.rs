#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
  use std::sync::atomic::{AtomicBool, Ordering};
  static STOP: AtomicBool = AtomicBool::new(false);
  extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Release);
  }
  let args = std::env::args_os().skip(1).collect::<Vec<_>>();
  if args.first().is_some_and(|arg| arg == "--read-file") {
    return guest::files::command(&args);
  }
  if args.first().is_some_and(|arg| arg == "--browse-files") {
    return guest::files::browse::command(&args);
  }
  if args.first().is_some_and(|arg| arg == "--create-file") {
    return guest::files::create::command(&args);
  }
  if args.first().is_some_and(|arg| arg == "--upload-file") {
    return guest::files::upload::command(&args);
  }
  anyhow::ensure!(
    args.is_empty() || args == ["--bootstrap"],
    "Invalid guest service mode"
  );
  for signal in [libc::SIGTERM, libc::SIGINT] {
    anyhow::ensure!(
      unsafe { libc::signal(signal, stop as *const () as libc::sighandler_t) } != libc::SIG_ERR,
      "Install guest service stop handler"
    );
  }
  if args.is_empty() {
    guest::daemon::run(&STOP)
  } else {
    guest::bootstrap::run(&STOP)
  }
}

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Guest service requires Linux");
}
