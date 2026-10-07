#!/usr/bin/env bash
# Build Hopper (release) and assemble dist/Hopper.app.
#
# The cargo bin target is `hopperdev` so a dev build never collides with an
# installed `hopper`; the shipped executable is `hopper`. Codesigns with
# CODESIGN_IDENTITY when set (a real Developer ID for a notarizable build),
# otherwise ad-hoc ("-") so it still runs locally.
#
# Lima's detached helper owns the Linux VM. Its virtualization entitlement
# stays on the helper; Hopper itself only needs files and networking.
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
profile="${HOPPER_BUILD_PROFILE:-release}"
case "$profile" in
  release) target_dir=target/release ;;
  dev) target_dir=target/debug ;;
  *) echo "error: HOPPER_BUILD_PROFILE must be release or dev" >&2; exit 1 ;;
esac

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
  echo "error: Hopper releases require an Apple silicon Mac" >&2
  exit 1
}

version="$(sed -n 's/^version = "\([0-9][^"]*\)".*/\1/p' Cargo.toml | head -1)"
[ -n "$version" ] || { echo "error: could not read version from Cargo.toml" >&2; exit 1; }
echo "[bundle] $app_name $version"

echo "[bundle] cargo build --profile $profile -p app -p mcp -p machine --locked"
cargo build --profile "$profile" -p app -p mcp -p machine --locked

mkdir -p dist
stage="$(mktemp -d dist/.hopper.XXXXXX)"
trap 'rm -rf "$stage"' EXIT
app="$stage/$app_name.app"
contents="$app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"

[ -x native/build/lima/bin/limactl ] || scripts/build/lima.sh
[ -f native/build/docker ] || scripts/build/docker.sh
[ -f native/build/compose ] || scripts/build/compose.sh
[ -x native/build/qemu/bin/swtpm ] || scripts/build/qemu.sh
[ -f native/build/windows/manifest.json ] || scripts/build/windows.sh
[ -f native/build/firmware/manifest.json ] && \
  [ -f native/build/firmware/windows.fd ] && \
  [ -f native/build/firmware/variables.fd ] && \
  [ -d native/build/firmware/licenses ] || scripts/build/firmware.sh
mkdir -p "$contents/Resources/firmware"
for artifact in windows.fd variables.fd manifest.json revision.txt license.txt; do
  cp "native/build/firmware/$artifact" "$contents/Resources/firmware/$artifact"
done
cp -R native/build/firmware/licenses "$contents/Resources/firmware/licenses"
cp -R native/build/qemu "$contents/Resources/qemu"
cp -R native/build/windows "$contents/Resources/windows"
mkdir -p "$contents/Resources/lima/bin" "$contents/Resources/lima/share/doc/lima"
cp native/build/lima/bin/limactl "$contents/Resources/lima/bin/limactl"
cp -R native/build/lima/share/lima "$contents/Resources/lima/share/"
cp native/build/lima/share/doc/lima/LICENSE "$contents/Resources/lima/share/doc/lima/"

cp "$target_dir/$src_bin" "$contents/MacOS/$bin_name"
[ -f assets/icon.icns ] && cp assets/icon.icns "$contents/Resources/icon.icns"

# The standalone Compose binary, so stacks work with no user-installed docker
# CLI. Found at runtime as `sidecars/compose` beside the executable.
if [ -f native/build/compose ]; then
  mkdir -p "$contents/MacOS/sidecars"
  cp native/build/compose "$contents/MacOS/sidecars/compose"
fi

# The Docker CLI is available for optional Docker-compatible engines. Apple's
# runtime has no Docker socket for this CLI to target.
if [ -f native/build/docker ]; then
  mkdir -p "$contents/MacOS/sidecars"
  cp native/build/docker "$contents/MacOS/sidecars/docker"
fi

# The MCP server is part of the release, too. Keeping it beside the app gives
# AI clients a stable executable path without requiring a global install.
mkdir -p "$contents/MacOS/sidecars"
cp "$target_dir/hoppermcp" "$contents/MacOS/sidecars/hoppermcp"
cp "$target_dir/hoppervm" "$contents/MacOS/sidecars/hoppervm"

# Never let a stale sidecar from another checkout or host architecture make it
# into a signed app. Universal binaries pass when they contain arm64.
if [ -d "$contents/MacOS/sidecars" ]; then
  required_arch=arm64
  while IFS= read -r sidecar; do
    [ -e "$sidecar" ] || continue
    [ -x "$sidecar" ] || {
      echo "error: sidecar is not executable: $sidecar" >&2
      exit 1
    }
    arches="$(lipo -archs "$sidecar" 2>/dev/null || true)"
    printf '%s\n' "$arches" | tr ' ' '\n' | grep -Fxq "$required_arch" || {
      echo "error: sidecar $sidecar does not contain host architecture $required_arch (has: ${arches:-unknown})" >&2
      exit 1
    }
  done < <(find "$contents/MacOS/sidecars" "$contents/Resources/lima/bin" "$contents/Resources/qemu/bin" "$contents/Resources/qemu/lib" "$contents/Resources/windows/bin" "$contents/Resources/windows/lib" -type f -perm -111)
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
	<string>26.0</string>
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
while IFS= read -r sidecar; do
  [ -e "$sidecar" ] || continue
  chmod u+w "$sidecar"
  if [ "$(basename "$sidecar")" = hoppervm ]; then
    codesign --force ${runtime_opts[@]+"${runtime_opts[@]}"} \
      --entitlements assets/machine.entitlements --sign "$identity" "$sidecar"
  else
    codesign --force ${runtime_opts[@]+"${runtime_opts[@]}"} \
      --preserve-metadata=entitlements --sign "$identity" "$sidecar"
  fi
done < <(find "$contents/MacOS/sidecars" "$contents/Resources/lima/bin" "$contents/Resources/qemu/bin" "$contents/Resources/qemu/lib" "$contents/Resources/windows/bin" "$contents/Resources/windows/lib" -type f -perm -111)
python3 scripts/build/windowsmanifest.py "$contents/Resources/windows"
python3 scripts/build/windowsmanifest.py --verify "$contents/Resources/windows"
codesign --force ${runtime_opts[@]+"${runtime_opts[@]}"} \
  --entitlements assets/hopper.entitlements \
  --sign "$identity" "$contents/MacOS/$bin_name"
codesign --force ${runtime_opts[@]+"${runtime_opts[@]}"} \
  --entitlements assets/hopper.entitlements \
  --sign "$identity" "$app"

codesign --verify --strict --verbose=2 "$app"
rm -rf "dist/$app_name.app"
mv "$app" "dist/$app_name.app"
echo "[bundle] -> dist/$app_name.app"
