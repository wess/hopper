#!/usr/bin/env python3
import json
import os
import struct
import sys
import time
from pathlib import Path


def read_exact(size):
    data = b""
    while len(data) < size:
        part = sys.stdin.buffer.read(size - len(data))
        if not part:
            sys.exit(0)
        data += part
    return data


def request():
    prefix = read_exact(8)
    assert prefix[:4] == b"HPV1"
    return json.loads(read_exact(struct.unpack("<I", prefix[4:])[0]))


def packet(id, result):
    header = json.dumps({"id": id, "result": result}).encode()
    return b"HPV1" + struct.pack("<I", len(header)) + header


def send(id, result):
    sys.stdout.buffer.write(packet(id, result))
    sys.stdout.buffer.flush()


start = request()
boot = start["command"]["boot"]
root = Path(boot["store"])
mode = boot["diskId"]
paused = False
(root / "pid").write_text(str(os.getpid()))
with (root / "boots").open("a") as boots:
    boots.write(json.dumps(boot) + "\n")
if boot.get("bootMedia") is None and (root / "systemreject").exists():
    send(start["id"], {"type": "rejected", "message": "synthetic system startup failure"})
    sys.exit(1)
if mode == "startupgate":
    while not (root / "startupcontinue").exists():
        time.sleep(0.005)
if mode == "startuphang":
    time.sleep(60)
if mode == "startupreject":
    send(start["id"], {"type": "rejected", "message": "fixture startup rejection"})
    time.sleep(60)
if mode == "startupwrong":
    send(start["id"], {"type": "accepted"})
    time.sleep(60)
send(start["id"], {"type": "started"})
if mode == "shutdown":
    send(0, {"type": "stopped", "reason": "shutdown"})
    sys.exit(0)
if mode == "exit":
    sys.exit(1)
while True:
    req = request()
    id = req["id"]
    kind = req["command"]["type"]
    event = root / "powerevent"
    if event.exists():
        reason = event.read_text()
        event.unlink()
        send(0, {"type": "stopped", "reason": reason})
        sys.exit(0)
    with (root / "trace").open("a") as trace:
        trace.write(kind + "\n")
    if kind == "input":
        with (root / "inputs").open("a") as inputs:
            inputs.write(json.dumps(req["command"]) + "\n")
    if kind == "stop":
        if mode == "stopdelay":
            (root / "stopreceived").touch()
            while not (root / "stopcontinue").exists():
                time.sleep(0.005)
        send(id, {"type": "stopped", "reason": "requested"})
        if mode == "stopdelay":
            while not (root / "exitcontinue").exists():
                time.sleep(0.005)
        sys.exit(0)
    if mode == "wrongid":
        send(id + 1, {"type": "accepted"})
        time.sleep(60)
    elif mode == "wrongtype":
        send(id, {"type": "running"})
        time.sleep(60)
    elif kind == "capture":
        result = {"type": "frame", "width": 1, "height": 1, "generation": 7}
        if mode == "oversized":
            result["width"] = 4097
            send(id, result)
            time.sleep(60)
        data = packet(id, result) + bytes([1, 2, 3, 255])
        if mode == "cancel":
            sys.stdout.buffer.write(data[:10])
            sys.stdout.buffer.flush()
            (root / "partial").touch()
            while not (root / "continue").exists():
                time.sleep(0.005)
            data = data[10:]
        for byte in data:
            sys.stdout.buffer.write(bytes([byte]))
            sys.stdout.buffer.flush()
    elif kind == "release" and mode == "reject":
        send(id, {"type": "rejected", "message": "fixture rejection"})
    elif kind == "status":
        status = root / "setupstatus"
        setup = json.loads(status.read_text()) if status.exists() else {"state": "waiting"}
        send(id, {"type": "status", "paused": paused, "setup": setup})
    elif kind == "pause":
        paused = True
        send(id, {"type": "paused"})
    elif kind == "resume":
        paused = False
        send(id, {"type": "running"})
    elif kind == "input" and mode == "inputdelay":
        (root / "inputreceived").touch()
        while not (root / "inputcontinue").exists():
            time.sleep(0.005)
        send(id, {"type": "accepted"})
    else:
        send(id, {"type": "accepted"})
