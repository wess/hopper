"""build an isolated Linux guest job diagnostic from a verified initramfs."""

import pathlib
import sys

import network

network.INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
failed() {
  echo HOPPER_JOBS_FAILED
  while :; do sleep 60; done
}
chmod 0755 /
mkdir -p /root /home /tmp /sys/fs/cgroup || failed
chmod 1777 /tmp || failed
chmod 0700 /root || failed
addgroup -g 1001 hopper || failed
adduser -D -u 1001 -G hopper hopper || failed
mount -t cgroup2 -o nosuid,nodev,noexec none /sys/fs/cgroup || failed
mkdir -m 0700 /sys/fs/cgroup/hopper || failed
/guestjobs || failed
while :; do sleep 60; done
'''

if __name__ == "__main__":
  if len(sys.argv) != 4:
    raise SystemExit("Provide verified initramfs, new output path and ARM64 guest job diagnostic")
  with pathlib.Path(sys.argv.pop()).open("rb") as file:
    helper = file.read((1 << 20) + 1)
  if (len(helper) < 64 or len(helper) > 1 << 20 or helper[:6] != b"\x7fELF\x02\x01"
      or int.from_bytes(helper[18:20], "little") != 183):
    raise ValueError("guest diagnostic must be a bounded ARM64 Linux executable")
  network.main((("guestjobs", helper),))
