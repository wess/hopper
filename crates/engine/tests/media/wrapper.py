#!/usr/bin/env python3
import os
import pathlib
import sys
import time

root = pathlib.Path(__file__).parent
if sys.argv[-1:] == ["products.xml"]:
  if (root / "gate").exists():
    (root / "waiting").touch()
    while not (root / "continue").exists():
      time.sleep(0.005)
  print((root / "catalog.xml").read_text())
else:
  original = (root / "original").read_text()
  os.execv(original, [original, *sys.argv[1:]])
