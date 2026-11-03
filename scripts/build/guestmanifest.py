import hashlib
import json
import pathlib
import struct
import sys


def executable(data):
  if not 64 <= len(data) <= 1024 * 1024 or data[:7] != b'\x7fELF\x02\x01\x01':
    raise ValueError('Guest executable must be bounded ARM64 ELF')
  kind, machine, version = struct.unpack_from('<HHI', data, 16)
  entry, offset = struct.unpack_from('<QQ', data, 24)
  header, width, count = struct.unpack_from('<HHH', data, 52)
  if (kind not in (2, 3) or machine != 183 or version != 1 or header != 64
      or width != 56 or not 1 <= count <= 128 or offset < 64
      or offset + count * width > len(data)):
    raise ValueError('Invalid ARM64 ELF program headers')
  code = False
  for index in range(count):
    kind, flags, start, address, _, size, memory, _ = struct.unpack_from(
      '<IIQQQQQQ', data, offset + index * width)
    if kind in (2, 3):
      raise ValueError('Guest executable must be statically linked')
    if start + size > len(data) or size > memory:
      raise ValueError('Invalid ELF segment bounds')
    if kind == 1 and flags & 1 and address <= entry < address + size:
      code = True
  if not code:
    raise ValueError('Guest executable has no entry in an executable segment')


def contents(root):
  root = pathlib.Path(root)
  required = {'hopperguest', 'hopperguest.service', 'licenses.html'}
  if root.is_symlink() or not root.is_dir():
    raise ValueError('Invalid guest asset directory')
  if {path.name for path in root.iterdir()} - {'manifest.json'} != required:
    raise ValueError('Incomplete or unexpected guest assets')
  files = {}
  for name in sorted(required):
    path = root / name
    if path.is_symlink() or not path.is_file():
      raise ValueError('Guest assets must be regular files')
    limit = {'hopperguest': 1024 * 1024, 'hopperguest.service': 4096,
             'licenses.html': 4 * 1024 * 1024}[name]
    if not 1 <= path.stat().st_size <= limit:
      raise ValueError('Guest asset exceeds bounds')
    data = path.read_bytes()
    if name == 'hopperguest':
      executable(data)
    else:
      data.decode('utf-8')
      if b'\0' in data:
        raise ValueError('Invalid guest service')
    files[name] = {'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
  return {'version': 1, 'target': 'aarch64-unknown-linux-musl', 'files': files}


def verify(root):
  root = pathlib.Path(root)
  path = root / 'manifest.json'
  if path.is_symlink() or not path.is_file() or path.stat().st_size > 4096:
    raise ValueError('Invalid guest manifest')
  if json.loads(path.read_text()) != contents(root):
    raise ValueError('Guest asset manifest mismatch')


if __name__ == '__main__':
  if len(sys.argv) == 3 and sys.argv[1] == '--verify':
    verify(sys.argv[2])
  elif len(sys.argv) == 2:
    root = pathlib.Path(sys.argv[1])
    manifest = contents(root)
    path = root / 'manifest.json'
    if path.is_symlink():
      raise ValueError('Guest manifest cannot be a symlink')
    path.write_text(json.dumps(manifest, indent=2) + '\n')
  else:
    raise SystemExit('usage: guestmanifest.py [--verify] DIRECTORY')
