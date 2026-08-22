#!/usr/bin/env bash
# Build Hopper (release) and assemble dist/Hopper.app.
#
# The cargo bin target is `hopperdev` so a dev build never collides with an
# installed `hopper`; the shipped executable is `hopper`. Codesigns with
# CODESIGN_IDENTITY when set (a real Developer ID for a notarizable build),
# otherwise ad-hoc ("-") so it still runs locally.
#
# There is no sidecar and no VM: on macOS the engine is Apple's `container`,
# installed separately and running under its own privileged helpers. So the app
# asks for no virtualization entitlement, and an ad-hoc build behaves like the
# signed one.
#
# Usage: scripts/bundle.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

app_name="Hopper"
src_bin="hopperdev"
bin_name="hopper"
bundle_id="io.wess.hopper"
identity="${CODESIGN_IDENTITY:--}"

version="$(sed -n 's/^version = "\([0-9][^"]*\)".*/\1/p' Cargo.toml | head -1)"
[ -n "$version" ] || { echo "error: could not read version from Cargo.toml" >&2; exit 1; }
echo "[bundle] $app_name $version"

echo "[bundle] cargo build --release -p app"
cargo build --release -p app

app="dist/$app_name.app"
contents="$app/Contents"
rm -rf "$app"
mkdir -p "$contents/MacOS" "$contents/Resources"

cp "target/release/$src_bin" "$contents/MacOS/$bin_name"
[ -f assets/icon.icns ] && cp assets/icon.icns "$contents/Resources/icon.icns"

# The standalone Compose binary, so stacks work with no user-installed docker
# CLI. Found at runtime as `sidecars/compose` beside the executable.
if [ -f native/build/compose ]; then
  mkdir -p "$contents/MacOS/sidecars"
  cp native/build/compose "$contents/MacOS/sidecars/compose"
fi

# The Docker CLI itself. Docker Desktop's uninstaller takes `docker` with it,
# so shipping one is what makes Hopper an actual replacement rather than a
# client that depends on the thing it replaces.
if [ -f native/build/docker ]; then
  mkdir -p "$contents/MacOS/sidecars"
  cp native/build/docker "$contents/MacOS/sidecars/docker"
fi

cat > "$contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>$app_name</string>
	<key>CFBundleDisplayName</key>
	<string>$app_name</string>
	<key>CFBundleIdentifier</key>
	<string>$bundle_id</string>
	<key>CFBundleExecutable</key>
	<string>$bin_name</string>
	<key>CFBundleIconFile</key>
	<string>icon</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$version</string>
	<key>CFBundleVersion</key>
	<string>$version</string>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.developer-tools</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
PLIST

# A real identity gets the hardened runtime (required for notarization);
# ad-hoc signing does not support it.
runtime_opts=()
if [ "$identity" != "-" ]; then
  runtime_opts=(--options runtime)
fi

echo "[bundle] codesign ($identity)"
for sidecar in "$contents/MacOS/sidecars/"*; do
  [ -e "$sidecar" ] || continue
  codesign --force ${runtime_opts[@]+"${runtime_opts[@]}"} \
    --sign "$identity" "$sidecar"
done
codesign --force ${runtime_opts[@]+"${runtime_opts[@]}"} \
  --entitlements assets/hopper.entitlements \
  --sign "$identity" "$contents/MacOS/$bin_name"
codesign --force ${runtime_opts[@]+"${runtime_opts[@]}"} \
  --entitlements assets/hopper.entitlements \
  --sign "$identity" "$app"

codesign --verify --strict --verbose=2 "$app" || true
echo "[bundle] -> $app"
