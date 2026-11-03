"""inspect selected system-driver metadata through read-only guest storage."""

import network

network.INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
failed() { echo HOPPER_INSPECT_FAILED; while :; do sleep 60; done; }
mkdir -p /modloop /target || failed
modprobe virtio_blk || failed
modprobe squashfs || failed
mount -t squashfs -o ro /dev/vdc /modloop || failed
mv /usr/lib/modules /usr/lib/modules.boot || failed
ln -s /modloop/modules /usr/lib/modules || failed
modprobe ntfs3 || failed
[ "$(cat /sys/class/block/vdb/ro)" = 1 ] || failed
blkid /dev/vdb* || failed
mount -t ntfs3 -o ro /dev/vdb3 /target || failed
for name in msseccore.sys ntoskrnl.exe; do
  case "$name" in
    *.sys) path=/target/Windows/System32/drivers/$name ;;
    *) path=/target/Windows/System32/$name ;;
  esac
  stat -c "%s %n" "$path" || failed
  sha256sum "$path" || failed
  head -c 256 "$path" | hexdump -C
done
umount /target || failed
echo HOPPER_INSPECT_DONE
while :; do sleep 60; done
'''

if __name__ == "__main__":
  network.main()
