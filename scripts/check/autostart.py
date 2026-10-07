"""build a disposable shutdown diagnostic, without installing an OS."""

import network

network.INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
failed() {
  printf '\nHOPPER-INSTALL:8197e0f0-0603-43e9-a817-eaf7ab0327af:failed\n'
  while :; do sleep 60; done
}
printf '\nHOPPER-INSTALL:8197e0f0-0603-43e9-a817-eaf7ab0327af:installing\n'
modprobe virtio_blk || failed
printf 'owned automatic handoff diagnostic' | dd of=/dev/vda bs=1 conv=notrunc 2>/dev/null || failed
sync
printf '\nHOPPER-INSTALL:8197e0f0-0603-43e9-a817-eaf7ab0327af:deployed\n'
poweroff -f
while :; do sleep 60; done
'''

if __name__ == "__main__":
  network.main()
