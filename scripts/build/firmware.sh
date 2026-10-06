#!/usr/bin/env bash
# build ARM64 UEFI from pinned source for the native Windows engine.
set -eo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/native/build/firmware"
source="$out/source"
revision=2970e5699ba6267f3384ffab20f96647578aebc8
tag=edk2-stable202608

for tool in git python3 make iasl; do
  command -v "$tool" >/dev/null || { echo "Missing firmware build tool: $tool" >&2; exit 1; }
done
llvm="${HOPPER_LLVM:-$(brew --prefix llvm)/bin}"
for tool in clang llvm-lib llvm-rc; do
  test -x "$llvm/$tool" || { echo "Missing LLVM tool: $llvm/$tool" >&2; exit 1; }
done
command -v lld-link >/dev/null || { echo 'Install LLVM lld for lld-link.' >&2; exit 1; }
mkdir -p "$out"
mkdir -p "$out/toolchain"
for tool in clang llvm-lib llvm-rc; do
  ln -sf "$llvm/$tool" "$out/toolchain/$tool"
done
ln -sf "$(command -v lld-link)" "$out/toolchain/lld-link"
if [ ! -d "$source" ]; then
  git clone --depth 1 --branch "$tag" https://github.com/tianocore/edk2.git "$source"
fi
test "$(git -C "$source" rev-parse HEAD)" = "$revision" || {
  echo "Firmware source must be at $revision; use a fresh build directory." >&2
  exit 1
}
git -C "$source" diff --quiet
git -C "$source" diff --cached --quiet
git -C "$source" submodule update --init --depth 1 \
  BaseTools/Source/C/BrotliCompress/brotli \
  MdeModulePkg/Library/BrotliCustomDecompressLib/brotli \
  MdePkg/Library/BaseFdtLib/libfdt \
  MdePkg/Library/MipiSysTLib/mipisyst \
  MdeModulePkg/Universal/RegularExpressionDxe/oniguruma \
  CryptoPkg/Library/OpensslLib/openssl \
  CryptoPkg/Library/MbedTlsLib/mbedtls \
  SecurityPkg/DeviceSecurity/SpdmLib/libspdm \
  TcgTpmPkg/Library/TpmLib/TPM

cd "$source"
python3 "$root/scripts/build/firmware.py" "$source" configure
export PYTHON_COMMAND=python3
export WORKSPACE="$source"
export EDK_TOOLS_PATH="$source/BaseTools"
export CLANG_BIN="$out/toolchain/"
export PATH="$llvm:$PATH"
export SOURCE_DATE_EPOCH="$(git show -s --format=%ct "$revision")"
. ./edksetup.sh --reconfig
set -u
make -C BaseTools/Source/C -j "${HOPPER_BUILD_JOBS:-4}"
# ArmVirt supplies the generic ARM platform drivers; execution is Hopper's.
build -a AARCH64 -t CLANGPDB -b DEBUG -p HopperPkg/firmware.dsc \
  -n "${HOPPER_BUILD_JOBS:-4}" -D FIRMWARE_VER=Hopper-UEFI-202608

image=Build/Hopper-AArch64/DEBUG_CLANGPDB/FV/QEMU_EFI.fd
variables=Build/Hopper-AArch64/DEBUG_CLANGPDB/FV/QEMU_VARS.fd
test -s "$image"
test -s "$variables"
cp "$image" "$out/windows.fd.tmp"
mv "$out/windows.fd.tmp" "$out/windows.fd"
cp "$variables" "$out/variables.fd.tmp"
mv "$out/variables.fd.tmp" "$out/variables.fd"
cp License.txt "$out/license.txt"
printf '%s\n' "$revision" > "$out/revision.txt"
python3 "$root/scripts/build/firmware.py" "$source"
echo "Firmware: $out/windows.fd"
