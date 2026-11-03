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
if start["command"]["type"] == "startGeneration":
    (root / "generationreceived").touch()
    while (root / "generationgate").exists():
        time.sleep(0.005)
if mode in ("startupgate", "restoredstartupgate", "generationstartupgate"):
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
if mode == "startupforeign":
    send(start["id"], {"type": "started", "machineId": "02020202-0202-0202-0202-020202020202", "configuration": [7] * 32})
    time.sleep(60)
if mode == "startupmissing":
    send(start["id"], {"type": "started"})
    time.sleep(60)
if mode == "startupzero":
    send(start["id"], {"type": "started", "machineId": boot["machineId"], "configuration": [0] * 32})
    time.sleep(60)
if mode == "startupconfigmissing":
    send(start["id"], {"type": "started", "machineId": boot["machineId"]})
    time.sleep(60)
if start["command"]["type"] in ("startRestored", "startGeneration"):
    paused = True
    kind = "generationStarted" if start["command"]["type"] == "startGeneration" else "restored"
    if mode == "generationwrongreply":
        kind = "restored"
    send(start["id"], {"type": kind, "machineId": boot["machineId"],
                      "configuration": json.loads((root / "configuration").read_text())
                      if (root / "configuration").exists() else
                      [7 if mode in ("restoredwrongconfig", "generationwrongconfig") else 3] * 32,
                      "restore": start["command"]["restore"]})
else:
    send(start["id"], {"type": "started", "machineId": boot["machineId"], "configuration": [7] * 32})
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
        if mode in ("stopdelay", "restoredstopdelay", "restoredresumedelay", "generationstopdelay", "generationresumedelay") or (root / "stopgate").exists():
            (root / "stopreceived").touch()
            while not (root / "stopcontinue").exists():
                time.sleep(0.005)
        send(id, {"type": "stopped", "reason": "requested"})
        (root / "stopacknowledged").touch()
        if mode in ("stopdelay", "restoredstopdelay", "restoredresumedelay", "generationstopdelay", "generationresumedelay") or (root / "stopgate").exists():
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
    elif kind == "finishDeployment":
        (root / "finishreceived").touch()
        send(id, {"type": "accepted"})
        while (root / "finishhold").exists():
            time.sleep(0.005)
        reason = "requested" if (root / "finishrequested").exists() else "shutdown"
        send(0, {"type": "stopped", "reason": reason})
        sys.exit(0)
    elif kind == "snapshotStart":
        (root / "snapshotidentity").write_text(req["command"]["snapshotId"])
        send(id, {"type": "rejected", "message": "capture unavailable"} if mode == "snapshotstartreject"
             else {"type": "accepted"})
    elif kind == "snapshotCancel":
        (root / "snapshotcancel").touch()
        if mode == "snapshotgates":
            while not (root / "cancelcontinue").exists():
                time.sleep(0.005)
        send(id, {"type": "rejected", "message": "cleanup unavailable"} if mode == "snapshotcancelreject"
             else {"type": "accepted"})
    elif kind == "snapshotPoll":
        snapshot = req["command"]["snapshotId"]
        (root / "snapshotpoll").touch()
        if mode == "snapshotsourceshutdown":
            send(0, {"type": "stopped", "reason": "shutdown"})
            sys.exit(0)
        if mode == "snapshotgates":
            while not (root / "pollcontinue").exists():
                time.sleep(0.005)
        if mode == "snapshotforeign":
            snapshot = "03030303-0303-0303-0303-030303030303"
        status = {"state": "capturing"}
        if mode == "snapshotmalformed":
            status = {"state": "ready", "receipt": {
                "nonce": [1] * 16, "device": 1, "inode": 2, "manifest": [3] * 31}}
        receipt = root / "snapshotreceipt"
        if receipt.exists():
            status = {"state": "ready", "receipt": json.loads(receipt.read_text())}
        elif mode == "snapshotcancelreject":
            status = {"state": "failed"}
        send(id, {"type": "snapshot", "snapshotId": snapshot, "status": status})
    elif kind == "status":
        status = root / "setupstatus"
        setup = json.loads(status.read_text()) if status.exists() else {"state": "waiting"}
        send(id, {"type": "status", "paused": paused, "setup": setup})
    elif kind == "pause":
        paused = True
        send(id, {"type": "paused"})
    elif kind in ("restoreResume", "restorePause"):
        if mode in ("restoredresumedelay", "generationresumedelay"):
            (root / "resumereceived").touch()
            while not (root / "resumecontinue").exists():
                time.sleep(0.005)
        paused = kind == "restorePause"
        send(id, {"type": "paused" if paused else "running"})
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
