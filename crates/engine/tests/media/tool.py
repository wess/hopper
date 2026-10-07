#!/usr/bin/env python3
import json
import pathlib
import sys
import time

root = pathlib.Path(__file__).parent
args = sys.argv[1:]
mode = (root / "mode").read_text()
if args[-1:] == ["products.xml"]:
  if mode == "gate":
    (root / "waiting").touch()
    while not (root / "continue").exists():
      time.sleep(0.005)
  print((root / "catalog.xml").read_text())
elif args[0] == "info":
  if "--xml" in args:
    edition = "Core" if mode == "invalid" else "Professional"
    print(f'<WIM><IMAGE INDEX="1"><WINDOWS><ARCH>12</ARCH><EDITIONID>{edition}</EDITIONID></WINDOWS></IMAGE></WIM>')
  else:
    print("Image Count: 6")
elif args[0] in ("apply", "export"):
  assert "--check" in args and "--quiet" in args
  with (root / "calls").open("a") as log:
    log.write(json.dumps(args[:3]) + "\n")
  if mode == "fail":
    sys.exit(1)
  target = pathlib.Path(args[3])
  if args[0] == "apply":
    assert args[2] == "1"
    (target / "sources").mkdir()
    (target / "efi/microsoft/boot").mkdir(parents=True)
    (target / "efi/microsoft/boot/efisys.bin").write_bytes(b"synthetic EFI")
    if mode == "existing":
      (target / "sources/boot.wim").write_bytes(b"unexpected destination")
  else:
    target.write_bytes(b"synthetic image")
    if args[2] == "3":
      assert "--boot" in args
else:
  assert "-o" not in args and "-udf" in args and "-no-emul-boot" in args
  files = pathlib.Path(args[-1])
  assert (files / "sources/install.wim").is_file()
  assert (files / "sources/boot.wim").is_file()
  sys.stdout.buffer.write(b"synthetic converted ISO")
