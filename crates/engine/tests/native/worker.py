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
(root / "pid").write_text(str(os.getpid()))
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
    with (root / "trace").open("a") as trace:
        trace.write(kind + "\n")
    if kind == "stop":
        send(id, {"type": "stopped", "reason": "requested"})
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
        send(id, {"type": "status", "paused": False, "setup": {"state": "waiting"}})
    elif kind == "pause":
        send(id, {"type": "paused"})
    elif kind == "resume":
        send(id, {"type": "running"})
    else:
        send(id, {"type": "accepted"})
