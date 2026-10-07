//! Image deployment for a new native Windows VM; never attach existing disks to this phase.

use anyhow::ensure;
use std::{
  fs::{File, OpenOptions},
  path::Path,
};

pub struct Layout {
  pub disk_gib: u32,
  pub image_index: u32,
  pub recovery_mib: u32,
  pub recovery_image_bytes: u64,
}

pub struct Plan {
  pub partitions: String,
  pub commands: String,
}

pub fn prepare(layout: &Layout) -> anyhow::Result<Plan> {
  ensure!(
    (64..=2048).contains(&layout.disk_gib),
    "Windows deployment needs a 64–2048 GiB disk"
  );
  ensure!(
    (1..=32).contains(&layout.image_index),
    "Invalid Windows image index"
  );
  ensure!(
    (1024..=4096).contains(&layout.recovery_mib),
    "Invalid Windows recovery partition size"
  );
  ensure!(
    layout.recovery_image_bytes > 0
      && layout.recovery_image_bytes.div_ceil(1024 * 1024) + 314 <= u64::from(layout.recovery_mib),
    "Recovery partition cannot hold the image and servicing space"
  );
  // leave room for both GPT headers and partition alignment; recovery consumes the remainder.
  let windows_mib = layout.disk_gib * 1024 - 512 - 16 - layout.recovery_mib - 2;
  let partitions = format!(
    "select disk 0\nclean\nconvert gpt\n\
create partition efi size=512\nformat quick fs=fat32 label=System\nassign letter=S\n\
create partition msr size=16\n\
create partition primary size={windows_mib}\nformat quick fs=ntfs label=Windows\nassign letter=W\n\
create partition primary\nformat quick fs=ntfs label=Recovery\nassign letter=R\n\
set id=de94bba4-06d1-4d40-a16a-bfd50179d6ac\ngpt attributes=0x8000000000000001\nexit\n"
  )
  .replace('\n', "\r\n");
  let commands = format!(r#"@echo off
setlocal
set "phase=checking deployment files"
set "step=0"
if not defined HOPPER_SETUP_PORT goto failed
call :phase
if errorlevel 1 goto failed
set "ambiguous="
set "driverfailed="
if not exist "%~dp0partitions.txt" goto failed
for %%F in (unattend.xml hopperspecialize.ps1 hopperfirstlogon.ps1) do if not exist "%~dp0%%F" goto failed
for %%D in (viostor vioscsi vioinput) do if not exist "%~dp0%%D\%%D.inf" goto failed
if not exist "%~dp0vioserial\vioser.inf" goto failed
set "phase=initializing Windows PE"
set "step=1"
call :phase
if errorlevel 1 goto failed
wpeinit
if errorlevel 1 goto failed
set "phase=loading signed guest drivers"
set "step=2"
call :phase
if errorlevel 1 goto failed
for %%D in (viostor vioscsi vioinput) do call :driver %%D
if defined driverfailed goto failed
set "phase=locating Windows installation media"
set "step=3"
call :phase
if errorlevel 1 goto failed
set "source="
for %%D in (C D E F G H I J K L M N O P Q R S T U V Y Z) do if exist "%%D:\sources\install.wim" call :source %%D:
if not defined source goto failed
if defined ambiguous goto failed
set "phase=validating Windows installation image"
set "step=4"
call :phase
if errorlevel 1 goto failed
dism /Get-WimInfo /WimFile:"%source%\sources\install.wim" /Index:{index}
if errorlevel 1 goto failed
set "phase=checking target drive letters"
set "step=5"
call :phase
if errorlevel 1 goto failed
if exist S:\ goto failed
if exist W:\ goto failed
if exist R:\ goto failed
set "phase=partitioning new disk"
set "step=6"
call :phase
if errorlevel 1 goto failed
echo Hopper deployment: %phase%
diskpart /s "%~dp0partitions.txt"
if errorlevel 1 goto failed
if not exist S:\ goto failed
if not exist W:\ goto failed
if not exist R:\ goto failed
set "phase=applying Windows image"
set "step=7"
call :phase
if errorlevel 1 goto failed
echo Hopper deployment: %phase%
dism /Apply-Image /ImageFile:"%source%\sources\install.wim" /Index:{index} /ApplyDir:W:\ /CheckIntegrity /Verify
if errorlevel 1 goto failed
set "phase=installing signed guest drivers"
set "step=8"
call :phase
if errorlevel 1 goto failed
echo Hopper deployment: %phase%
dism /Image:W:\ /Add-Driver /Driver:"%~dp0viostor\viostor.inf"
if errorlevel 1 goto failed
dism /Image:W:\ /Add-Driver /Driver:"%~dp0vioscsi\vioscsi.inf"
if errorlevel 1 goto failed
dism /Image:W:\ /Add-Driver /Driver:"%~dp0vioinput\vioinput.inf"
if errorlevel 1 goto failed
dism /Image:W:\ /Add-Driver /Driver:"%~dp0vioserial\vioser.inf"
if errorlevel 1 goto failed
set "phase=staging first-boot provisioning"
set "step=9"
call :phase
if errorlevel 1 goto failed
echo Hopper deployment: %phase%
if not exist W:\Windows\Panther mkdir W:\Windows\Panther
if errorlevel 1 goto failed
copy /y "%~dp0unattend.xml" W:\Windows\Panther\unattend.xml
if errorlevel 1 goto failed
icacls W:\Windows\Panther\unattend.xml /inheritance:r /grant:r "*S-1-5-18:F" "*S-1-5-32-544:F"
if errorlevel 1 goto failed
if not exist W:\Windows\Setup\Scripts mkdir W:\Windows\Setup\Scripts
if errorlevel 1 goto failed
copy /y "%~dp0hopperspecialize.ps1" W:\Windows\Setup\Scripts\hopperspecialize.ps1
if errorlevel 1 goto failed
copy /y "%~dp0hopperfirstlogon.ps1" W:\Windows\Setup\Scripts\hopperfirstlogon.ps1
if errorlevel 1 goto failed
set "phase=configuring recovery"
set "step=10"
call :phase
if errorlevel 1 goto failed
echo Hopper deployment: %phase%
if not exist W:\Windows\System32\Recovery\winre.wim goto failed
mkdir R:\Recovery\WindowsRE
if errorlevel 1 goto failed
copy /y W:\Windows\System32\Recovery\winre.wim R:\Recovery\WindowsRE\winre.wim
if errorlevel 1 goto failed
W:\Windows\System32\reagentc /setreimage /path R:\Recovery\WindowsRE /target W:\Windows
if errorlevel 1 goto failed
set "phase=configuring UEFI boot"
set "step=11"
call :phase
if errorlevel 1 goto failed
echo Hopper deployment: %phase%
W:\Windows\System32\bcdboot W:\Windows /s S: /f UEFI
if errorlevel 1 goto failed
cmd /c exit 0
>"\\.\org.hopper.setup" echo HOPPERSETUP/1 deployed 11
if errorlevel 1 goto failed
echo Hopper deployment: image ready for first-boot provisioning
exit /b 0
:phase
cmd /c exit 0
>"\\.\org.hopper.setup" echo HOPPERSETUP/1 phase %step%
exit /b %errorlevel%
:source
if defined source set "ambiguous=1"
set "source=%~1"
exit /b 0
:driver
drvload "%~dp0%~1\%~1.inf"
if errorlevel 1 set "driverfailed=1"
exit /b 0
:failed
if defined HOPPER_SETUP_PORT >"\\.\org.hopper.setup" echo HOPPERSETUP/1 failed %step%
echo Hopper deployment failed while %phase%. The VM has not been marked ready.
exit /b 1
"#, index = layout.image_index).replace('\n', "\r\n");
  Ok(Plan {
    partitions,
    commands,
  })
}

/// The runtime must expose this new, locked file as its only writable guest disk.
/// Existing files are rejected rather than reused for destructive deployment.
pub fn create_disk(path: &Path, layout: &Layout) -> anyhow::Result<File> {
  prepare(layout)?;
  let mut options = OpenOptions::new();
  options.read(true).write(true).create_new(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  let file = options.open(path)?;
  fs2::FileExt::try_lock_exclusive(&file)?;
  file.set_len(u64::from(layout.disk_gib) * 1024 * 1024 * 1024)?;
  Ok(file)
}
