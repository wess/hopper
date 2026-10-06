#!/usr/bin/env bash
# Bundle the ARM64 Windows runtime and relocate its non-system libraries.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
command -v brew >/dev/null || { echo 'Homebrew is required to assemble the Windows runtime.' >&2; exit 1; }
brew fetch --force-bottle --deps qemu swtpm wimlib cdrtools
python3 "$root/scripts/build/qemu.py" "$root/native/build/qemu"
