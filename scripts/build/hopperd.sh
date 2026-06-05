#!/usr/bin/env bash
# Build the hopperd VM helper (release) and sign it with the virtualization
# entitlement so it can boot a VM. Set HOPPER_SIGN_IDENTITY to a real identity
# (e.g. "Apple Development: you (TEAMID)" or "Developer ID Application: …") for
# a bootable local build — an ad-hoc signature (the default) carries the
# entitlement but macOS will NOT honor it, so the VM won't start.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root/native/hopperd"

swift build -c release
bin=".build/release/hopperd"

identity="${HOPPER_SIGN_IDENTITY:--}"
codesign --force --options runtime --timestamp \
  --entitlements hopperd.entitlements -s "$identity" "$bin"

if [ "$identity" = "-" ]; then
  echo "built + AD-HOC signed (VM won't boot): native/hopperd/$bin"
  echo "  set HOPPER_SIGN_IDENTITY to a real Apple identity for a bootable build."
else
  echo "built + signed ($identity): native/hopperd/$bin"
fi
