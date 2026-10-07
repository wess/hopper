"""build a disposable network diagnostic from a verified Alpine initramfs."""

import gzip
import io
import pathlib
import sys


INIT = rb'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/busybox --install -s /usr/bin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
exec >/dev/hvc0 2>&1
failed() {
  echo HOPPER_NETWORK_FAILED
  while :; do sleep 60; done
}
modprobe virtio_net || failed
if grep -q 'hopper.seed=1' /proc/cmdline; then
  modprobe virtio_blk || failed
  modprobe isofs || failed
  mkdir -p /media/seed
  mount -t iso9660 -o ro /dev/vdb /media/seed || failed
  [ "$(cat /sys/class/block/vdb/ro)" = 1 ] || failed
  grep -q '^#cloud-config' /media/seed/user-data || failed
  grep -q 'instance-id: hopper-' /media/seed/meta-data || failed
  grep -q 'username: hopperadmin' /media/seed/user-data || failed
  grep -q 'name: hopper' /media/seed/user-data || failed
  grep -q '\$6\$rounds=100000\$' /media/seed/user-data || failed
  echo HOPPER_SEED_OK
fi
iface=
for device in /sys/class/net/*; do
  [ "${device##*/}" = lo ] || iface=${device##*/}
done
[ -n "$iface" ] || failed
ip link set "$iface" up || failed
sleep 2
if grep -q 'hopper.network=offline' /proc/cmdline; then
  [ "$(cat /sys/class/net/$iface/carrier)" = 0 ] || failed
  echo HOPPER_NETWORK_OFFLINE_OK
else
  udhcpc -i "$iface" -n -q -t 5 -T 2 -s /dhcp || failed
  mkdir -p /tmp
  wget -T 15 -O /tmp/releases http://dl-cdn.alpinelinux.org/alpine/v3.24/releases/aarch64/latest-releases.yaml || failed
  grep -q 'flavor: alpine-virt' /tmp/releases || failed
  echo HOPPER_NETWORK_NAT_OK
fi
while :; do sleep 60; done
'''

DHCP = b'''#!/bin/sh
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
case "$1" in
  bound|renew)
    ip addr flush dev "$interface"
    ip addr add "$ip/$mask" dev "$interface" || exit 1
    set -- $router
    ip route replace default via "$1" dev "$interface" || exit 1
    : > /etc/resolv.conf
    for server in $dns; do echo "nameserver $server" >> /etc/resolv.conf; done
    ;;
esac
'''


def entry(fields, name, data):
  fields = list(fields)
  name = name.encode() + b"\0"
  fields[6] = len(data)
  fields[11] = len(name)
  header = b"070701" + b"".join(f"{value:08x}".encode() for value in fields)
  result = header + name
  result += b"\0" * (-len(result) % 4)
  result += data
  result += b"\0" * (-len(result) % 4)
  return result


def main():
  if len(sys.argv) != 3:
    raise SystemExit("Provide verified Alpine initramfs and a new diagnostic output path")
  source, target = map(pathlib.Path, sys.argv[1:])
  if not source.is_file() or source.stat().st_size > 64 << 20:
    raise ValueError("initramfs must be a regular file no larger than 64 MiB")
  with gzip.GzipFile(fileobj=io.BytesIO(source.read_bytes())) as compressed:
    raw = compressed.read((128 << 20) + 1)
  if len(raw) > 128 << 20:
    raise ValueError("uncompressed initramfs exceeds 128 MiB")
  offset = 0
  output = bytearray()
  replaced = 0
  while offset < len(raw):
    if raw[offset] == 0:
      offset += 1
      continue
    if raw[offset:offset + 6] != b"070701":
      raise ValueError("expected newc initramfs")
    fields = [int(raw[offset + 6 + i * 8:offset + 14 + i * 8], 16) for i in range(13)]
    size, namesize = fields[6], fields[11]
    start = offset + 110
    end = start + namesize
    if not 0 < namesize <= 4096 or end > len(raw) or raw[end - 1] != 0:
      raise ValueError("invalid initramfs name")
    name = raw[start:end - 1].decode()
    if pathlib.PurePosixPath(name).is_absolute() or ".." in pathlib.PurePosixPath(name).parts:
      raise ValueError("invalid initramfs path")
    start = (end + 3) & ~3
    end = start + size
    if end > len(raw):
      raise ValueError("truncated initramfs")
    data = raw[start:end]
    offset = (end + 3) & ~3
    if name == "TRAILER!!!":
      continue
    if name.removeprefix("./") == "init":
      data = INIT
      fields[1] = 0o100755
      replaced += 1
    output.extend(entry(fields, name, data))
  if replaced != 1:
    raise ValueError("expected one init entry")
  fields = [0, 0o100755, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
  output.extend(entry(fields, "dhcp", DHCP))
  fields[1] = 0
  output.extend(entry(fields, "TRAILER!!!", b""))
  with target.open("xb") as file:
    file.write(gzip.compress(output, mtime=0))
  target.chmod(0o600)
  print("Created disposable network diagnostic initramfs")


if __name__ == "__main__":
  main()
