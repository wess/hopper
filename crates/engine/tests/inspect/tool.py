#!/usr/bin/env python3
import pathlib
import shutil
import sys
import time

root = pathlib.Path(__file__).parent
args = sys.argv[1:]
mode = (root / "mode").read_text()
if args[:2] == ["image", "attach"]:
  assert "--readOnly" in args and "--nobrowse" in args and "--plist" in args
  point = pathlib.Path(args[args.index("--mountPoint") + 1])
  (root / "point").write_text(str(point))
  (point / "sources").mkdir(parents=True)
  (point / "sources/install.wim").write_bytes(b"readonly image")
  print("mounted")
elif args[0] == "eject":
  point = pathlib.Path(args[1])
  assert point == pathlib.Path((root / "point").read_text())
  if mode == "ejectfail":
    sys.exit(1)
  shutil.rmtree(point)
  (root / "ejected").touch()
elif args[0] == "info":
  if mode == "hang":
    (root / "waiting").touch()
    time.sleep(60)
  if mode == "invalid":
    print("invalid metadata")
  else:
    print('<WIM><IMAGE INDEX="7"><WINDOWS><ARCH>12</ARCH><EDITIONID>Professional</EDITIONID></WINDOWS></IMAGE></WIM>')
elif args[0] == "dir":
  assert args[2] == "7"
  if "--detailed" in args:
    assert "--path=/Windows/System32/Recovery/Winre.wim" in args
    assert "--one-file-only" in args
    print("Uncompressed size = 1200000000 bytes")
  else:
    print("/Windows/System32/Recovery/Winre.wim")
else:
  sys.exit(2)
