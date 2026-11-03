"""package cross-compiled guest test harnesses for execution in an isolated Linux VM."""

import gzip
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

import network

network.INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
export HOPPER_GUEST_TEST_EXECUTABLE=/hopperguest
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
failed() { echo HOPPER_TESTS_FAILED; while :; do sleep 60; done; }
mkdir -p /tmp /tests /root /home || failed
mount -t tmpfs -o nosuid,nodev,mode=1777,size=64m tmpfs /tmp || failed
chmod 0755 / /tests /home || failed
chmod 0700 /root || failed
addgroup -g 1001 hopper || failed
adduser -D -u 1001 -G hopper hopper || failed
for name in bootstrap browse guest upload files create; do
  gzip -dc "/$name.gz" > "/tests/$name" || failed
  chmod 0755 "/tests/$name" || failed
  echo "HOPPER_TEST_HARNESS root $name"
  "/tests/$name" --test-threads=1 || failed
  echo "HOPPER_TEST_HARNESS normal $name"
  su -s /bin/sh hopper -c "HOPPER_GUEST_TEST_EXECUTABLE=/hopperguest /tests/$name --test-threads=1" || failed
done
echo HOPPER_TESTS_OK
while :; do sleep 60; done
'''

if __name__ == "__main__":
  if len(sys.argv) != 5:
    raise SystemExit("Provide verified initramfs, new output path, Cargo JSON artifact log and production helper")
  source, target, log, helper = map(pathlib.Path, sys.argv[1:])
  strip = shutil.which("llvm-strip")
  if not strip:
    raise ValueError("llvm-strip is required")
  executable = helper.read_bytes()
  if not 64 <= len(executable) <= 1 << 20 or executable[:7] != b"\x7fELF\x02\x01\x01" or int.from_bytes(executable[18:20], "little") != 183:
    raise ValueError("production helper must be bounded ARM64 Linux ELF")
  expected = {"bootstrap", "browse", "guest", "upload", "files", "create"}
  artifacts = {}
  for line in log.read_text().splitlines():
    value = json.loads(line)
    name = value.get("target", {}).get("name")
    if value.get("reason") == "compiler-artifact" and value.get("profile", {}).get("test") and name in expected and value.get("executable"):
      if name in artifacts:
        raise ValueError("ambiguous guest test artifact")
      artifacts[name] = pathlib.Path(value["executable"])
  if artifacts.keys() != expected:
    raise ValueError("missing guest test harnesses")
  payload = [("hopperguest", executable)]
  with tempfile.TemporaryDirectory(prefix="hopperguesttests.") as staging:
    for name, artifact in sorted(artifacts.items()):
      output = pathlib.Path(staging) / name
      shutil.copyfile(artifact, output)
      subprocess.run([strip, str(output)], check=True)
      data = output.read_bytes()
      if not 64 <= len(data) <= 4 << 20 or data[:7] != b"\x7fELF\x02\x01\x01" or int.from_bytes(data[18:20], "little") != 183:
        raise ValueError("test harness must be bounded ARM64 Linux ELF")
      payload.append((name + ".gz", gzip.compress(data, mtime=0)))
    sys.argv = [sys.argv[0], str(source), str(target)]
    network.main(payload)
