"""build a disposable folder-sharing diagnostic from a verified Alpine initramfs."""

import network

network.INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
failed() {
  echo HOPPER_SHARING_FAILED
  while :; do sleep 60; done
}
modprobe virtiofs || failed
mkdir -p /shares
mount -t virtiofs hopper /shares || failed
[ "$(cat /shares/writable/sentinel)" = authorized-writable ] || failed
[ "$(cat /shares/readonly/sentinel)" = authorized-readonly ] || failed
printf guest-write > /shares/writable/created || failed
if printf forbidden > /shares/readonly/created; then failed; fi
if cat /shares/writable/escape; then failed; fi
if cat /shares/writable/relative; then failed; fi
echo HOPPER_SHARING_OK
while [ ! -e /shares/readonly/check ]; do sleep 1; done
value=$(cat /shares/writable/sentinel 2>/dev/null)
[ "$value" != replacement-scope ] || failed
printf replacement-write > /shares/writable/after 2>/dev/null || true
echo HOPPER_REPLACEMENT_OK
while :; do sleep 60; done
'''

if __name__ == "__main__":
  network.main()
