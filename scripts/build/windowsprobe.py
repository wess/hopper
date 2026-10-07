"""Build native Windows PE media for driver diagnostics or new-disk deployment."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import stat
import struct
import subprocess


FILES = {
  "viostor": ["viostor.inf", "viostor.cat", "viostor.sys"],
  "vioinput": ["vioinput.inf", "vioinput.cat", "vioinput.sys", "viohidkmdf.sys"],
  "vioscsi": ["vioscsi.inf", "vioscsi.cat", "vioscsi.sys"],
  "vioserial": ["vioser.inf", "vioser.cat", "vioser.sys"],
}
DEPLOYMENT = (
  "partitions.txt", "deploy.cmd", "unattend.xml", "hopperspecialize.ps1", "hopperfirstlogon.ps1",
)


def deployment_files(folder):
  info = folder.lstat()
  if not stat.S_ISDIR(info.st_mode) or info.st_mode & 0o077:
    raise ValueError("Deployment directory must be private")
  files = {}
  for name in DEPLOYMENT:
    path = folder / name
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o077:
      raise ValueError(f"Deployment file must be private: {name}")
    if not 0 < info.st_size <= 256 * 1024:
      raise ValueError(f"Invalid deployment file size: {name}")
    files[name] = path.read_bytes()
  return files


def validate(path):
  data = path.read_bytes()
  if not data:
    raise ValueError(f"Empty driver file: {path}")
  if path.suffix == ".sys":
    if len(data) < 64 or data[:2] != b"MZ":
      raise ValueError(f"Invalid driver executable: {path}")
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset + 6 > len(data) or data[offset:offset + 4] != b"PE\0\0":
      raise ValueError(f"Invalid driver PE header: {path}")
    if struct.unpack_from("<H", data, offset + 4)[0] != 0xaa64:
      raise ValueError(f"Driver is not ARM64: {path}")
  elif path.suffix == ".inf" and b"NTARM64" not in data.upper():
    raise ValueError(f"Driver INF has no ARM64 section: {path}")
  return hashlib.sha256(data).hexdigest()


def bootstrap(deployment=False, serial=False):
  if deployment:
    return (
      "@echo off\r\n"
      "title Hopper native Windows deployment\r\n"
      "call X:\\hopper\\deploy.cmd\r\n"
      "if errorlevel 1 goto failed\r\n"
      "echo Hopper image prepared. Stop the VM and boot from its new disk.\r\n"
      "goto finished\r\n"
      ":failed\r\n"
      "echo Hopper deployment stopped. The VM has not been marked ready.\r\n"
      ":finished\r\n"
      "cmd /k\r\n"
    )
  commands = (
    "@echo off\r\n"
    "title Hopper native Windows driver diagnostic\r\n"
    "wpeinit\r\n"
    "echo Loading signed ARM64 storage driver...\r\n"
    "drvload X:\\hopper\\viostor\\viostor.inf\r\n"
    "echo Storage driver exit code: %errorlevel%\r\n"
    "echo Loading signed ARM64 optical transport driver...\r\n"
    "drvload X:\\hopper\\vioscsi\\vioscsi.inf\r\n"
    "echo Optical driver exit code: %errorlevel%\r\n"
    "echo Loading signed ARM64 input driver...\r\n"
    "drvload X:\\hopper\\vioinput\\vioinput.inf\r\n"
    "echo Input driver exit code: %errorlevel%\r\n"
    "echo list disk > X:\\hopper\\disks.txt\r\n"
    "echo list volume >> X:\\hopper\\disks.txt\r\n"
    "diskpart /s X:\\hopper\\disks.txt\r\n"
    "echo Driver diagnostic finished. No installation was started.\r\n"
  )
  if serial:
    commands += (
      "drvload X:\\hopper\\vioserial\\vioser.inf\r\n"
      "if errorlevel 1 goto serialfailed\r\n"
      "for /L %%I in (1,1,10) do (\r\n"
      "  echo hopper native setup channel>\\\\.\\org.hopper.setup\r\n"
      "  if not errorlevel 1 goto serialdone\r\n"
      "  ping 127.0.0.1 -n 2 >nul\r\n"
      ")\r\n"
      ":serialfailed\r\n"
      "echo Hopper guest serial diagnostic failed.\r\n"
      "goto finished\r\n"
      ":serialdone\r\n"
      "echo Hopper guest serial diagnostic sent its marker.\r\n"
      ":finished\r\n"
    )
  return commands + "cmd /k\r\n"


def build(installer, drivers, out, mkisofs, deployment=None, serial=False):
  if deployment is not None and serial:
    raise ValueError("Serial diagnostic and deployment are separate boot modes")
  staged = deployment_files(deployment) if deployment is not None else {}
  hashes = {}
  for name, files in FILES.items():
    for file in files:
      hashes[f"{name}/{file}"] = validate(drivers / name / "w11/ARM64" / file)
  license = drivers.parents[2] / "doc/virtio-win/virtio-win_license.txt"
  if not license.is_file():
    raise ValueError("Driver package license is missing")
  # wimlib's command language quotes paths; reject characters it cannot represent here.
  if any(c in str(out.resolve()) for c in '\"\r\n'):
    raise ValueError("Output path contains unsupported characters")
  out.mkdir(mode=0o700, parents=True, exist_ok=False)
  media = out / "media"
  media.mkdir()
  subprocess.run([
    "bsdtar", "-xf", str(installer), "-C", str(media),
    "--include", "EFI/*", "--include", "BOOT/*", "--include", "SOURCES/BOOT.WIM",
    "--include", "BOOTMGR.EFI",
  ], check=True)
  # UDF lookup in the firmware is case-sensitive; match Microsoft's boot paths.
  for path in sorted(media.rglob("*"), key=lambda p: len(p.parts), reverse=True):
    path.rename(path.with_name(path.name.lower()))
  payload = out / "payload"
  payload.mkdir()
  for name, files in FILES.items():
    (payload / name).mkdir()
    for file in files:
      shutil.copyfile(drivers / name / "w11/ARM64" / file, payload / name / file)
  shutil.copyfile(license, payload / "license.txt")
  for name, data in staged.items():
    path = payload / name
    path.write_bytes(data)
    path.chmod(0o600)
  (payload / "drivers.cmd").write_bytes(bootstrap(bool(staged), serial).encode("ascii"))
  shell = out / "winpeshl.ini"
  shell.write_bytes(
    b"[LaunchApps]\r\n%SYSTEMROOT%\\System32\\cmd.exe, /c X:\\hopper\\drivers.cmd\r\n"
  )
  commands = (
    f'add "{payload.resolve()}" /hopper\n'
    f'add "{shell.resolve()}" /Windows/System32/winpeshl.ini\n'
  )
  boot = media / "sources/boot.wim"
  boot.chmod(boot.stat().st_mode | 0o200)
  subprocess.run([
    "wimlib-imagex", "update", str(boot), "2",
  ], input=commands, text=True, check=True)
  image = out / ("deployment.iso" if staged else "drivers.iso")
  subprocess.run([
    str(mkisofs), "-udf", "-iso-level", "3", "-V", "HOPPERSETUP" if staged else "HOPPERDRIVERS",
    "-eltorito-platform", "efi", "-b", "efi/microsoft/boot/efisys.bin",
    "-no-emul-boot", "-o", str(image), str(media),
  ], check=True)
  image.chmod(0o600)
  (out / "manifest.json").write_text(json.dumps({
    "purpose": "new-disk native Windows deployment" if staged else
      "boot-only native driver diagnostic, no Windows installation",
    "driverSha256": hashes,
    "containsGuestCredentials": bool(staged),
    "detachBeforeFirstBoot": bool(staged),
  }, indent=2) + "\n")
  print(image)


def main():
  parser = argparse.ArgumentParser(description=__doc__)
  parser.add_argument("installer", type=Path)
  parser.add_argument("drivers", type=Path, help="Extracted virtio-win drivers/by-driver directory")
  parser.add_argument("output", type=Path, help="New diagnostic directory; existing paths are refused")
  parser.add_argument("--mkisofs", default="mkisofs", help="UDF-capable mkisofs executable")
  parser.add_argument("--deployment", type=Path,
    help="Private generated deployment bundle; only attach a new writable target as disk 0")
  parser.add_argument("--serial-check", action="store_true", help="Send a bounded guest serial marker")
  args = parser.parse_args()
  build(args.installer.resolve(), args.drivers.resolve(), args.output.resolve(), args.mkisofs,
    args.deployment, args.serial_check)


if __name__ == "__main__":
  main()
