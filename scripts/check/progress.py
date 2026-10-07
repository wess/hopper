"""build a disposable serial progress diagnostic, without installing an OS."""

import network

network.INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
printf '\nHOPPER-INSTALL:8197e0f0-0603-43e9-a817-eaf7ab0327af:installing\n'
dd if=/dev/zero bs=8192 count=128 2>/dev/null
printf '\nHOPPER-INSTALL:8197e0f0-0603-43e9-a817-eaf7ab0327af:deployed\n'
while :; do sleep 60; done
'''

if __name__ == "__main__":
  network.main()
