"""inspect a stopped Ubuntu disk through read-only VZ storage and a no-replay mount."""

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
modprobe ext4 || failed
[ "$(cat /sys/class/block/vdb/ro)" = 1 ] || failed
blkid /dev/vdb* || failed
mount -t ext4 -o ro,noload /dev/vdb2 /target || failed
ls -l /target/usr/libexec/hopperguest /target/etc/systemd/system/hopperguest.service /target/etc/systemd/system/multi-user.target.wants/hopperguest.service /target/etc/udev/rules.d/99hopperboot.rules
stat -c "%u %g %a %s %n" /target/usr/libexec/hopperguest /target/usr/share/doc/hopperguest/licenses.html
sha256sum /target/usr/libexec/hopperguest /target/usr/share/doc/hopperguest/licenses.html
ls /target/var/lib/cloud/instances/*/boot-finished /target/var/lib/cloud/instances/*/sem/config_scripts_user
chroot /target /usr/bin/journalctl --no-pager -u hopperguest.service -n 40
chroot /target /usr/bin/journalctl --no-pager -u cloud-final.service -n 15
for log in /target/var/log/cloud-init.log /target/var/log/cloud-init-output.log; do
  grep -F hopperguest "$log" | cut -c 1-512 | tail -n 12
done
umount /target || failed
echo HOPPER_INSPECT_DONE
while :; do sleep 60; done
'''
if __name__ == "__main__":
  network.main()
