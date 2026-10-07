"""build a disposable folder-sharing diagnostic from a verified Alpine initramfs."""

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
  echo HOPPER_SHARING_FAILED
  while :; do sleep 60; done
}
chmod 0755 /
modprobe virtiofs || failed
if grep -q 'hopper.sharing=absent' /proc/cmdline; then
  if /mountshares check; then failed; fi
  [ ! -e /mnt/hopper ] || failed
  echo HOPPER_SHARING_ABSENT_OK
  while :; do sleep 60; done
fi
/mountshares check || failed
mount -t tmpfs tmpfs /sys/fs || failed
/mountshares check || failed
umount /sys/fs || failed
mkdir -p /mnt /run
ln -s /tmp /mnt/hopper || failed
if /mountshares; then failed; fi
rm /mnt/hopper || failed
mkdir /mnt/hopper || failed
mount -t tmpfs tmpfs /mnt/hopper || failed
if /mountshares; then failed; fi
umount /mnt/hopper || failed
ln -s /tmp /run/hopper-shares || failed
if /mountshares; then failed; fi
rm /run/hopper-shares || failed
mkdir -m 0755 /run/hopper-shares || failed
chmod 0777 /run/hopper-shares || failed
if /mountshares; then failed; fi
chmod 0755 /run/hopper-shares || failed
printf keep > /run/hopper-shares/foreign || failed
if /mountshares; then failed; fi
[ "$(cat /run/hopper-shares/foreign)" = keep ] || failed
rm /run/hopper-shares/foreign || failed
ln -s /tmp /run/hopper-shares/writable || failed
if /mountshares; then failed; fi
[ "$(readlink /run/hopper-shares/writable)" = /tmp ] || failed
rm /run/hopper-shares/writable || failed
ln -s /mnt/hopper/removed /run/hopper-shares/removed || failed
/mountshares || failed
/mountshares || failed
[ ! -L /run/hopper-shares/removed ] || failed
[ "$(stat -c '%u:%g:%a' /run/hopper-shares)" = 0:0:755 ] || failed
[ "$(readlink /run/hopper-shares/writable)" = /mnt/hopper/writable ] || failed
[ "$(readlink /run/hopper-shares/readonly)" = /mnt/hopper/readonly ] || failed
[ "$(readlink /run/hopper-shares/.hidden)" = /mnt/hopper/.hidden ] || failed
grep -q 'hopper /mnt/hopper virtiofs .*nosuid.*nodev' /proc/mounts || failed
[ "$(cat /mnt/hopper/writable/sentinel)" = authorized-writable ] || failed
[ "$(cat /mnt/hopper/readonly/sentinel)" = authorized-readonly ] || failed
printf guest-write > /mnt/hopper/writable/created || failed
if printf forbidden > /mnt/hopper/readonly/created; then failed; fi
if cat /mnt/hopper/writable/escape; then failed; fi
if cat /mnt/hopper/writable/relative; then failed; fi
/identity 1001 /usr/bin/sh -c '
  [ "$(id -u)" = 1001 ] && [ "$(id -g)" = 1001 ] || exit 1
  if /mountshares; then exit 1; fi
  grep -q "^CapEff:[[:space:]]*0000000000000000$" /proc/self/status || exit 1
  ls -l /run/hopper-shares >/dev/null || exit 1
  [ "$(cat /run/hopper-shares/writable/sentinel)" = authorized-writable ] || exit 1
  [ "$(cat /run/hopper-shares/readonly/sentinel)" = authorized-readonly ] || exit 1
  [ "$(cat /run/hopper-shares/.hidden/sentinel)" = authorized-readonly ] || exit 1
  if ln -s /tmp /run/hopper-shares/injected; then exit 1; fi
  [ "$(cat /mnt/hopper/writable/sentinel)" = authorized-writable ] || exit 1
  [ "$(cat /mnt/hopper/readonly/sentinel)" = authorized-readonly ] || exit 1
  printf guest-user-write > /mnt/hopper/writable/usercreated || exit 1
  if printf forbidden > /mnt/hopper/readonly/usercreated; then exit 1; fi
  if cat /mnt/hopper/writable/escape; then exit 1; fi
  if cat /mnt/hopper/writable/relative; then exit 1; fi
' || failed
echo HOPPER_USER_SHARING_OK
echo HOPPER_SHARING_OK
while [ ! -e /mnt/hopper/readonly/check ]; do sleep 1; done
value=$(cat /mnt/hopper/writable/sentinel 2>/dev/null)
[ "$value" != replacement-scope ] || failed
printf replacement-write > /mnt/hopper/writable/after 2>/dev/null || true
echo HOPPER_REPLACEMENT_OK
while :; do sleep 60; done
'''

if __name__ == "__main__":
  if len(sys.argv) != 4:
    raise SystemExit("Provide verified initramfs, new output path and ARM64 diagnostic identity helper")
  with pathlib.Path(sys.argv.pop()).open("rb") as file:
    helper = file.read((1 << 20) + 1)
  if (len(helper) < 64 or len(helper) > 1 << 20 or helper[:6] != b"\x7fELF\x02\x01"
      or int.from_bytes(helper[18:20], "little") != 183):
    raise ValueError("identity helper must be a bounded Linux ELF executable")
  script = pathlib.Path(__file__).resolve().parents[2] / "crates/engine/src/machines/linux/sharing/mount.sh"
  network.main((("identity", helper), ("mountshares", script.read_bytes())))
