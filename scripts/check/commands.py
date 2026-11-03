"""package the production Linux daemon for an isolated private-port diagnostic."""

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
  echo HOPPER_COMMANDS_FAILED
  while :; do sleep 60; done
}
chmod 0755 /
mkdir -p /root /home /tmp /run /sys/fs/cgroup /modloop || failed
mount -t tmpfs -o nosuid,nodev,mode=0755,size=16m tmpfs /home || failed
mkdir -p /usr/libexec || failed
ln /hopperguest /usr/libexec/hopperguest || failed
chmod 1777 /tmp || failed
chmod 0700 /root || failed
addgroup -g 1001 hopper || failed
adduser -D -u 1001 -G hopper hopper || failed
mount -t tmpfs -o nosuid,nodev,mode=0755 tmpfs /run || failed
mount -t cgroup2 -o nosuid,nodev,noexec none /sys/fs/cgroup || failed
modprobe virtio_blk || failed
modprobe squashfs || failed
mount -t squashfs -o ro /dev/vdb /modloop || failed
mv /usr/lib/modules /usr/lib/modules.boot || failed
ln -s /modloop/modules /usr/lib/modules || failed
modprobe virtio_console || failed
modprobe vmw_vsock_virtio_transport || failed
/hopperguest --bootstrap || failed
[ "$(stat -c '%a %s' /run/hopper/boot)" = '400 61' ] || failed
su -s /bin/sh hopper -c 'test ! -r /run/hopper/boot' || failed
found=0
for device in /sys/class/virtio-ports/*; do
  [ -f "$device/name" ] || continue
  [ "$(cat "$device/name")" = hopper.boot ] || continue
  node="/dev/${device##*/}"
  [ "$(stat -c '%a' "$node")" = 600 ] || failed
  su -s /bin/sh hopper -c "test ! -r '$node'" || failed
  found=$((found + 1))
done
[ "$found" = 1 ] || failed
echo HOPPER_COMMANDS_PRIVATE_OK
/hopperguest || failed
while :; do sleep 60; done
'''

if __name__ == '__main__':
  if len(sys.argv) != 4:
    raise SystemExit('Provide verified initramfs, new output path and production ARM64 daemon')
  with pathlib.Path(sys.argv.pop()).open('rb') as source:
    helper = source.read((1 << 20) + 1)
  if (not 64 <= len(helper) <= 1 << 20 or helper[:7] != b'\x7fELF\x02\x01\x01'
      or int.from_bytes(helper[18:20], 'little') != 183):
    raise ValueError('daemon must be a bounded ARM64 Linux executable')
  network.main((('hopperguest', helper),))
