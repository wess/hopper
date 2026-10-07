#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
command -v brew >/dev/null || { echo 'Homebrew is required to assemble media tools.' >&2; exit 1; }
brew fetch --force-bottle --deps wimlib cdrtools
python3 "$root/scripts/build/windows.py" "$root/native/build/windows"
