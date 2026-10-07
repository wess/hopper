#!/usr/bin/env python3
import os
from pathlib import Path
import sys
import time

root = Path(__file__).parent
mode = (root / "mode").read_text()
name = Path(__file__).name
if name == "archive":
    if sys.argv[1] == "-tf":
        if mode == "hang":
            (root / "pid").write_text(str(os.getpid()))
            time.sleep(60)
        if mode == "flood":
            (root / "pid").write_text(str(os.getpid()))
            sys.stdout.buffer.write(b"x" * (2 * 1024 * 1024))
            sys.stdout.buffer.flush()
            time.sleep(60)
        print("EFI/MICROSOFT/BOOT/EFISYS.BIN\nSOURCES/BOOT.WIM\nBOOTMGR.EFI")
    elif sys.argv[1] == "-tvf":
        print("- regular EFI/MICROSOFT/BOOT/EFISYS.BIN\n- regular SOURCES/BOOT.WIM\n- regular BOOTMGR.EFI")
    else:
        sys.stdout.buffer.write(b"synthetic boot media")
elif name == "wim":
    commands = sys.stdin.read()
    assert '/hopper' in commands and '/Windows/System32/winpeshl.ini' in commands
    assert sys.argv[1] == "update" and sys.argv[3] == "2"
    (root / "updated").touch()
    if mode == "fail":
        sys.exit(3)
elif name == "image":
    image = Path(sys.argv[sys.argv.index("-o") + 1])
    assert (root / "updated").exists()
    image.write_bytes(b"completed private image")
