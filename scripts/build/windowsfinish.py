"""cross-compile the freestanding ARM64 installer handshake helper."""

import pathlib
import os
import shutil
import subprocess
import sys
import tempfile


def build(destination):
  destination = pathlib.Path(destination)
  project = pathlib.Path(__file__).resolve().parents[2]
  imports = ["CreateFileA", "CreateEventA", "WriteFile", "ReadFile", "GetOverlappedResult",
             "GetLastError", "WaitForSingleObject", "CancelIoEx", "CloseHandle", "ExitProcess",
             "ResetEvent", "GetTickCount64"]
  llvm = os.environ.get("HOPPER_LLVM")
  if not llvm and shutil.which("brew"):
    llvm = str(pathlib.Path(subprocess.check_output(["brew", "--prefix", "llvm"], text=True).strip()) / "bin")
  clang = str(pathlib.Path(llvm) / "clang") if llvm else shutil.which("clang")
  dlltool = str(pathlib.Path(llvm) / "llvm-dlltool") if llvm else shutil.which("llvm-dlltool")
  linker = shutil.which("lld-link")
  if not all((clang, dlltool, linker)):
    raise RuntimeError("LLVM clang, llvm-dlltool and lld-link are required")
  destination.mkdir(parents=True, exist_ok=True)
  with tempfile.TemporaryDirectory(dir=destination) as temporary:
    stage = pathlib.Path(temporary)
    (stage / "kernel32.def").write_text("LIBRARY KERNEL32.dll\nEXPORTS\n" + "\n".join(imports) + "\n")
    subprocess.run([dlltool, "-m", "arm64", "-d", str(stage / "kernel32.def"),
                    "-l", str(stage / "kernel32.lib")], check=True, timeout=60)
    subprocess.run([clang, "--target=aarch64-pc-windows-msvc", "-ffreestanding",
                    "-fno-stack-protector", "-O2", "-Wall", "-Wextra", "-Werror", "-c",
                    str(project / "native/guest/finish.c"), "-o", str(stage / "finish.obj")],
                   check=True, timeout=60)
    output = stage / "hopperfinish.exe"
    subprocess.run([linker, "/entry:entry", "/subsystem:console", "/nodefaultlib", "/machine:arm64",
                    "/timestamp:0", f"/out:{output}", str(stage / "finish.obj"),
                    str(stage / "kernel32.lib")], check=True, timeout=60)
    data = output.read_bytes()
    offset = int.from_bytes(data[60:64], "little")
    if not (70 <= len(data) <= 1 << 20 and data[:2] == b"MZ"
            and data[offset:offset + 6] == b"PE\0\0\x64\xaa"):
      raise RuntimeError("Invalid ARM64 installer helper")
    guest = destination / "guest"
    guest.mkdir(exist_ok=True)
    output.chmod(0o644)
    output.replace(guest / "hopperfinish.exe")
    licenses = destination / "licenses"
    licenses.mkdir(exist_ok=True)
    shutil.copyfile(project / "LICENSE", licenses / "hopper.txt")
    (licenses / "hopper.txt").chmod(0o644)


if __name__ == "__main__":
  build(sys.argv[1])
