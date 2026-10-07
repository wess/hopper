#!/usr/bin/env python3
import pathlib
import plistlib
import subprocess
import sys


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
  if (resources / 'qemu').exists():
    raise ValueError('Previous Windows runtime must not be bundled')
  for path in app.rglob('*'):
    if path.is_symlink() and not path.resolve().is_relative_to(app.resolve()):
      raise ValueError(f'Bundle symlink escapes app: {path}')
    if path.name.lower().startswith(('qemu-', 'swtpm')):
      raise ValueError(f'Forbidden runtime artifact: {path}')
  for relative in ['MacOS/hopper', 'MacOS/sidecars/hoppermcp', 'MacOS/sidecars/hoppervm',
                   'MacOS/sidecars/docker', 'MacOS/sidecars/compose',
                   'Resources/lima/bin/limactl', 'Resources/windows/bin/wimlib-imagex',
                   'Resources/windows/bin/mkisofs']:
    binary = contents / relative
    if not binary.is_file() or not binary.stat().st_mode & 0o111:
      raise ValueError(f'Missing executable: {relative}')
    if b'arm64' not in run('lipo', '-archs', str(binary)).split():
      raise ValueError(f'Missing ARM64 code: {relative}')
    run('codesign', '--verify', '--strict', str(binary))
  entitlement(contents / 'MacOS/hopper', 'com.apple.security.virtualization')
  entitlement(contents / 'MacOS/sidecars/hoppervm', 'com.apple.security.hypervisor')
  entitlement(resources / 'lima/bin/limactl', 'com.apple.security.virtualization')
  run(sys.executable, 'scripts/build/windowsmanifest.py', '--verify', str(resources / 'windows'))
  for name in ['windows.fd', 'variables.fd', 'manifest.json', 'revision.txt', 'license.txt']:
    if not (resources / 'firmware' / name).is_file():
      raise ValueError(f'Missing owned firmware: {name}')
  run('codesign', '--verify', '--deep', '--strict', str(app))
  print('Bundle verified: owned desktop runtimes, ARM64 sidecars and required entitlements')


if __name__ == '__main__':
  verify(sys.argv[1])
