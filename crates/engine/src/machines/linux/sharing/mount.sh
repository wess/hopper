#!/bin/sh
set -eu

[ "$(id -u)" = 0 ] || exit 1
if [ "${1:-}" = check ] && [ "$#" = 1 ]; then
  if [ -d /sys/fs/virtiofs ]; then
    for tag in /sys/fs/virtiofs/*/tag; do
      if [ -f "$tag" ] && [ "$(cat "$tag")" = hopper ]; then
        exit 0
      fi
    done
    exit 1
  fi
  # older kernels expose the bound driver without a tag attribute.
  for device in /sys/bus/virtio/drivers/virtiofs/virtio*; do
    [ ! -d "$device" ] || exit 0
  done
  exit 1
fi
[ "$#" = 0 ] || exit 64
[ ! -L /mnt/hopper ] || exit 1
mkdir -p /mnt/hopper
if mountpoint -q /mnt/hopper; then
  awk '$1 == "hopper" && $2 == "/mnt/hopper" && $3 == "virtiofs" &&
    $4 ~ /(^|,)nosuid(,|$)/ && $4 ~ /(^|,)nodev(,|$)/ { found = 1 }
    END { exit !found }' /proc/mounts
  exit
fi
exec mount -t virtiofs -o nosuid,nodev hopper /mnt/hopper
