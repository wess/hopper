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
else
  mount -t virtiofs -o nosuid,nodev hopper /mnt/hopper
fi

# file managers cannot query all metadata on Apple's synthetic share root.
# keep their index on the guest filesystem and link only named directories.
index=/run/hopper-shares
valid_name() {
  case "$1" in ""|.|..|*[!a-zA-Z0-9._-]*) return 1 ;; esac
  [ "${#1}" -le 64 ]
}
[ ! -L "$index" ] || exit 1
if [ ! -e "$index" ]; then
  (umask 022; mkdir -m 0755 "$index")
fi
[ -d "$index" ] && [ "$(stat -c '%u:%g:%a' "$index")" = 0:0:755 ] || exit 1

count=0
for path in /mnt/hopper/* /mnt/hopper/.[!.]* /mnt/hopper/..?*; do
  [ -e "$path" ] || [ -L "$path" ] || continue
  name=${path##*/}
  valid_name "$name" && [ -d "$path" ] && [ ! -L "$path" ] || exit 1
  count=$((count + 1))
  [ "$count" -le 16 ] || exit 1
done
count=0
for link in "$index"/* "$index"/.[!.]* "$index"/..?*; do
  [ -e "$link" ] || [ -L "$link" ] || continue
  name=${link##*/}
  valid_name "$name" && [ -L "$link" ] || exit 1
  [ "$(stat -c '%u:%g' "$link")" = 0:0 ] || exit 1
  [ "$(readlink "$link")" = "/mnt/hopper/$name" ] || exit 1
  count=$((count + 1))
  [ "$count" -le 16 ] || exit 1
done
for link in "$index"/* "$index"/.[!.]* "$index"/..?*; do
  [ -L "$link" ] || continue
  [ -d "/mnt/hopper/${link##*/}" ] || rm "$link"
done
for path in /mnt/hopper/* /mnt/hopper/.[!.]* /mnt/hopper/..?*; do
  [ -d "$path" ] || continue
  name=${path##*/}
  [ -L "$index/$name" ] || ln -s "$path" "$index/$name"
done
