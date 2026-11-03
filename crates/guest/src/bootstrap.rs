#[cfg(target_os = "linux")]
mod device;
#[cfg(target_os = "linux")]
mod publish;

pub fn port(name: &str) -> bool {
  let Some(numbers) = name.strip_prefix("vport") else {
    return false;
  };
  let Some((device, index)) = numbers.split_once('p') else {
    return false;
  };
  [device, index].iter().all(|value| {
    !value.is_empty()
      && value.len() <= 10
      && value.bytes().all(|byte| byte.is_ascii_digit())
      && (value.len() == 1 || !value.starts_with('0'))
      && value.parse::<u32>().is_ok()
  })
}

pub fn number(value: &str) -> anyhow::Result<(u32, u32)> {
  let value = value.strip_suffix('\n').unwrap_or(value);
  let (major, minor) = value
    .split_once(':')
    .ok_or_else(|| anyhow::anyhow!("Invalid boot device identity"))?;
  anyhow::ensure!(
    [major, minor].iter().all(|part| !part.is_empty()
      && part.len() <= 10
      && part.bytes().all(|byte| byte.is_ascii_digit())
      && (part.len() == 1 || !part.starts_with('0'))),
    "Invalid boot device identity"
  );
  Ok((major.parse()?, minor.parse()?))
}

#[cfg(target_os = "linux")]
pub fn run(stop: &std::sync::atomic::AtomicBool) -> anyhow::Result<()> {
  anyhow::ensure!(
    unsafe {
      libc::getuid() == 0 && libc::geteuid() == 0 && libc::getgid() == 0 && libc::getegid() == 0
    },
    "Boot credential delivery requires root"
  );
  let directory = publish::directory()?;
  if publish::exists(&directory)? {
    return Ok(());
  }
  let material = device::read(stop)?;
  publish::write(&directory, &material, stop)
}
