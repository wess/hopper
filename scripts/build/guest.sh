#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
target=aarch64-unknown-linux-musl
linker="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER:-ld.lld}"
command -v "$linker" >/dev/null || {
  echo 'error: install lld or set CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER' >&2
  exit 1
}
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$linker"
export CARGO_PROFILE_RELEASE_STRIP=symbols
cargo build -p guest --bin hopperguest --target "$target" --release --locked

mkdir -p native/build
stage="$(mktemp -d native/build/.guest.XXXXXX)"
trap 'rm -rf "$stage"' EXIT
cp "target/$target/release/hopperguest" "$stage/hopperguest"
cp crates/guest/assets/hopperguest.service "$stage/hopperguest.service"
python3 scripts/build/guestlicenses.py "$stage/licenses.html"
chmod 0644 "$stage/licenses.html"
chmod 0755 "$stage/hopperguest"
chmod 0644 "$stage/hopperguest.service"
python3 scripts/build/guestmanifest.py "$stage"
python3 scripts/build/guestmanifest.py --verify "$stage"
mkdir -p native/build/guesttools
cp "$stage/hopperguest" "$stage/hopperguest.service" "$stage/manifest.json" "$stage/licenses.html" native/build/guesttools/
python3 scripts/build/guestmanifest.py --verify native/build/guesttools
