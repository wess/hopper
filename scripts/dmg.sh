#!/usr/bin/env bash
# Package dist/Hopper.app into dist/Hopper.dmg. Run scripts/bundle.sh first.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

app="dist/Hopper.app"
dmg="dist/Hopper.dmg"
[ -d "$app" ] || { echo "error: $app not found — run scripts/bundle.sh first" >&2; exit 1; }

rm -f "$dmg"
staging="$(mktemp -d)"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"

hdiutil create -volname "Hopper" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null
rm -rf "$staging"
echo "[dmg] -> $dmg"
