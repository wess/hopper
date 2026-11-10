#!/usr/bin/env python3
import gzip
import os
import pathlib
import plistlib
import subprocess
import sys
import tempfile


def run(*args):
  return subprocess.check_output(args, stderr=subprocess.STDOUT)


def entitlement(binary, key):
  data = run('codesign', '-d', '--entitlements', '-', '--xml', str(binary))
  start = data.find(b'<?xml')
  if start < 0:
    raise ValueError(f'Missing entitlements: {binary}')
  end = data.index(b'</plist>', start) + len(b'</plist>')
  if plistlib.loads(data[start:end]).get(key) is not True:
    raise ValueError(f'Missing {key}: {binary}')


def verify(app):
  app = pathlib.Path(app)
  contents = app / 'Contents'
  resources = contents / 'Resources'
  for path in app.rglob('*'):
    if path.is_symlink() and not path.resolve().is_relative_to(app.resolve()):
      raise ValueError(f'Bundle symlink escapes app: {path}')
  for relative in ['MacOS/hopper', 'MacOS/sidecars/hoppermcp',
                   'MacOS/sidecars/docker', 'MacOS/sidecars/compose', 'MacOS/sidecars/buildx',
                   'MacOS/sidecars/docker-credential-osxkeychain',
                   'Resources/lima/bin/limactl']:
    binary = contents / relative
    if not binary.is_file() or not binary.stat().st_mode & 0o111:
      raise ValueError(f'Missing executable: {relative}')
    if b'arm64' not in run('lipo', '-archs', str(binary)).split():
      raise ValueError(f'Missing ARM64 code: {relative}')
    run('codesign', '--verify', '--strict', str(binary))
  entitlement(resources / 'lima/bin/limactl', 'com.apple.security.virtualization')
  darwin_guest_archive = resources / 'lima/share/lima/lima-guestagent.Darwin-aarch64.gz'
  with tempfile.NamedTemporaryFile() as darwin_guest:
    with gzip.open(darwin_guest_archive, 'rb') as source:
      darwin_guest.write(source.read())
    darwin_guest.flush()
    pathlib.Path(darwin_guest.name).chmod(0o755)
    if b'arm64' not in run('lipo', '-archs', darwin_guest.name).split():
      raise ValueError('Lima Darwin guest agent is not ARM64')
    run('codesign', '--verify', '--strict', darwin_guest.name)
    if os.environ.get('CODESIGN_IDENTITY', '-') != '-':
      details = run('codesign', '-d', '--verbose=4', darwin_guest.name)
      if b'runtime' not in details or b'Timestamp=' not in details:
        raise ValueError('Lima Darwin guest agent lacks hardened runtime or timestamp')
  license = resources / 'LICENSE'
  if (license.is_symlink() or not license.is_file()
      or license.read_bytes() != pathlib.Path('LICENSE').read_bytes()):
    raise ValueError('Missing or mismatched project license')
  credential_notice = resources / 'licenses' / 'docker-credential-helpers.txt'
  if (credential_notice.is_symlink() or not credential_notice.is_file()
      or credential_notice.read_bytes() != pathlib.Path('assets/licenses/docker-credential-helpers.txt').read_bytes()):
    raise ValueError('Missing or mismatched Docker credential helper license')
  run('codesign', '--verify', '--deep', '--strict', str(app))
  print('Bundle verified: ARM64 sidecars, the VM helper and required entitlements')


if __name__ == '__main__':
  verify(sys.argv[1])
