"""build a disposable guest-initiated virtio socket diagnostic."""

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
  echo HOPPER_SOCKET_FAILED
  while :; do sleep 60; done
}
modprobe virtio_blk || failed
modprobe squashfs || failed
mkdir -p /modloop || failed
mount -t squashfs -o ro /dev/vdb /modloop || failed
mv /usr/lib/modules /usr/lib/modules.boot || failed
ln -s /modloop/modules /usr/lib/modules || failed
modprobe vmw_vsock_virtio_transport || failed
/socketcheck || failed
while :; do sleep 60; done
'''

if __name__ == "__main__":
  if len(sys.argv) != 4:
    raise SystemExit("Provide verified initramfs, new output path and ARM64 socket fixture")
  with pathlib.Path(sys.argv.pop()).open("rb") as file:
    helper = file.read((1 << 20) + 1)
  if (len(helper) < 64 or len(helper) > 1 << 20 or helper[:6] != b"\x7fELF\x02\x01"
      or int.from_bytes(helper[18:20], "little") != 183):
    raise ValueError("socket fixture must be a bounded ARM64 Linux executable")
  network.main((("socketcheck", helper),))
