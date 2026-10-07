"""build an audio diagnostic from the verified Alpine kernel's initramfs."""

import network

network.INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
failed() {
  echo HOPPER_AUDIO_FAILED
  while :; do sleep 60; done
}
modprobe virtio_blk || failed
modprobe squashfs || failed
mkdir -p /modloop || failed
mount -t squashfs -o ro /dev/vdb /modloop || failed
mv /usr/lib/modules /usr/lib/modules.boot || failed
ln -s /modloop/modules /usr/lib/modules || failed
modprobe virtio_snd || failed
sleep 1
if grep -q 'hopper.audio=disabled' /proc/cmdline; then
  [ ! -e /dev/snd/pcmC0D0p ] || failed
  if [ -e /proc/asound/pcm ] && grep -q playback /proc/asound/pcm; then failed; fi
  echo HOPPER_AUDIO_DISABLED_OK
else
  cat /proc/asound/pcm || failed
  grep -q playback /proc/asound/pcm || failed
  if grep -q capture /proc/asound/pcm; then failed; fi
  [ -c /dev/snd/pcmC0D0p ] || failed
  [ ! -e /dev/snd/pcmC0D0c ] || failed
  echo HOPPER_AUDIO_ENABLED_OK
fi
while :; do sleep 60; done
'''

if __name__ == "__main__":
  network.main()
