#!/bin/sh
# Cargo runner for local macOS development.
#
# Hopper's managed VM engine needs the `com.apple.security.virtualization`
# entitlement to start. The release .app gets it from bundle.sh; a plain
# `cargo run` binary has no signature and so no entitlement, and the managed
# engine dead-ends with nothing to run. This runner signs the dev binary
# ad-hoc with that entitlement before launching it — enough for local use — so
# `cargo run -p app` brings up a real engine (it downloads the guest image once
# and boots it) instead of telling you to go install the signed app.
#
# Only the app dev binary is signed; test harnesses and the MCP server run
# untouched, so `cargo test` is unaffected. A failed sign is non-fatal — the
# app just falls back to whatever engine you already run.
#
# Wired in via .cargo/config.toml. Not used by CI or the release, which build
# and sign through bundle.sh.
set -e

bin="$1"
shift

case "${bin##*/}" in
  hopperdev | hopper)
    root="$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
    codesign --force --sign - \
      --entitlements "$root/assets/hopper.entitlements" \
      "$bin" 2>/dev/null || true
    ;;
esac

exec "$bin" "$@"
