"""Record firmware provenance and retain upstream notices beside the image."""

import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys


def git(source, *args):
  return subprocess.check_output(["git", "-C", str(source), *args], text=True).rstrip()


def configure(source):
  profile = (source / "ArmVirtPkg/ArmVirtQemu.dsc").read_text()
  for field, value in {
    "PLATFORM_NAME": "Hopper",
    "PLATFORM_GUID": "c7179281-6978-4488-af8c-499e572f09ca",
    "OUTPUT_DIRECTORY": "Build/Hopper-AArch64",
  }.items():
    profile, count = re.subn(
      rf"^(\s*{field}\s*=\s*)[^\n]+", rf"\g<1>{value}", profile, flags=re.MULTILINE,
    )
    if count != 1:
      raise RuntimeError(f"Expected one upstream {field} definition")
  for library in ["QemuFwCfgMmioDxeLib", "QemuFwCfgMmioPeiLib"]:
    old = f"OvmfPkg/Library/QemuFwCfgLib/{library}.inf"
    if profile.count(old) != 1:
      raise RuntimeError(f"Expected one upstream {library} instance")
    profile = profile.replace(old, "OvmfPkg/Library/QemuFwCfgLib/QemuFwCfgLibNull.inf")
  target = source / "HopperPkg/firmware.dsc"
  target.parent.mkdir(exist_ok=True)
  target.write_text(profile)


def main():
  source = Path(sys.argv[1]).resolve()
  if len(sys.argv) == 3 and sys.argv[2] == "configure":
    configure(source)
    return
  out = source.parent
  notices = out / "licenses"
  notices.mkdir(exist_ok=True)
  modules = []
  for line in git(source, "submodule", "status").splitlines():
    if line.startswith("-"):
      continue
    if not line.startswith(" "):
      raise RuntimeError("Firmware submodule differs from its pinned revision")
    revision, path, *_ = line.split()
    modules.append({"path": path, "revision": revision})
    module = source / path
    for name in git(module, "ls-files").splitlines():
      file = module / name
      if file.name.lower().startswith(("license", "copying", "notice")) and file.is_file():
        target = notices / path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(file, target)
  for name in ["License.txt", "License-History.txt"]:
    shutil.copyfile(source / name, notices / name.lower())

  image = out / "windows.fd"
  variables = out / "variables.fd"
  manifest = {
    "source": "https://github.com/tianocore/edk2.git",
    "revision": git(source, "rev-parse", "HEAD"),
    "epoch": int(git(source, "show", "-s", "--format=%ct", "HEAD")),
    "platform": "HopperPkg/firmware.dsc",
    "profileSha256": hashlib.sha256((source / "HopperPkg/firmware.dsc").read_bytes()).hexdigest(),
    "target": "DEBUG",
    "toolchain": "CLANGPDB",
    "clang": subprocess.check_output([str(out / "toolchain/clang"), "--version"], text=True),
    "linker": subprocess.check_output([str(out / "toolchain/lld-link"), "--version"], text=True),
    "submodules": modules,
    "size": image.stat().st_size,
    "sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
    "variables": {
      "size": variables.stat().st_size,
      "sha256": hashlib.sha256(variables.read_bytes()).hexdigest(),
    },
  }
  staging = out / "manifest.json.tmp"
  staging.write_text(json.dumps(manifest, indent=2) + "\n")
  staging.replace(out / "manifest.json")


if __name__ == "__main__":
  main()
