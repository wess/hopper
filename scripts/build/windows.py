#!/usr/bin/env python3
import pathlib
import shutil
import subprocess
import sys
import tempfile
import urllib.request

import bottles
import windowsmanifest as manifest


def drivers(package, stage):
  if not package.is_file() or package.stat().st_size > 350 * 1024 * 1024:
    raise ValueError('Invalid driver package')
  if manifest.digest(package) != manifest.SHA256:
    raise ValueError('Driver package checksum mismatch')
  listing = subprocess.check_output(['/usr/bin/tar', '-tf', str(package)], timeout=120)
  selected = members(listing.decode())
  # include each hard-link group, but never unpack unrelated architectures or tools.
  with tempfile.TemporaryDirectory(dir=stage.parent) as temporary:
    extracted = pathlib.Path(temporary)
    subprocess.check_call(
      ['/usr/bin/tar', '-xf', str(package), '-C', str(extracted), '--', *selected], timeout=120,
    )
    sources = [('usr/share/doc/virtio-win/virtio-win_license.txt', 'licenses/virtio.txt')]
    sources.extend((
      f'usr/share/virtio-win/drivers/by-driver/{group}/w11/ARM64/{name}',
      f'drivers/{group}/w11/ARM64/{name}',
    ) for group, names in manifest.DRIVERS.items() for name in names)
    for source, relative in sources:
      original = extracted / source
      if (original.is_symlink() or not original.is_file()
          or not 1 <= original.stat().st_size <= 8 * 1024 * 1024):
        raise ValueError(f'Invalid driver member: {source}')
      output = stage / relative
      output.parent.mkdir(parents=True, exist_ok=True)
      shutil.copyfile(original, output)
      output.chmod(0o644)


def members(listing):
  if len(listing) > 4 * 1024 * 1024:
    raise ValueError('Oversized driver package catalogue')
  selected = []
  for name in listing.splitlines():
    normalized = name.removeprefix('./')
    parts = pathlib.PurePosixPath(normalized).parts
    driver = (len(parts) == 9
              and parts[:5] == ('usr', 'share', 'virtio-win', 'drivers', 'by-driver')
              and parts[5] in manifest.DRIVERS and parts[7] == 'ARM64'
              and parts[8] in manifest.DRIVERS[parts[5]])
    by_os = (len(parts) == 8
             and parts[:6] == ('usr', 'share', 'virtio-win', 'drivers', 'by-os', 'ARM64')
             and parts[7] in {name for files in manifest.DRIVERS.values() for name in files})
    if driver or by_os or normalized == 'usr/share/doc/virtio-win/virtio-win_license.txt':
      if normalized != '/'.join(parts) or any(part in ('..', '.') for part in parts) or name in selected:
        raise ValueError('Invalid driver member path')
      selected.append(name)
  if not 14 <= len(selected) <= 128:
    raise ValueError('Incomplete or oversized driver member selection')
  return selected


def download(target):
  with urllib.request.urlopen(manifest.URL, timeout=60) as response, target.open('xb') as output:
    total = 0
    while chunk := response.read(1024 * 1024):
      total += len(chunk)
      if total > 350 * 1024 * 1024:
        raise ValueError('Oversized driver package')
      output.write(chunk)


def main(destination, package=None):
  destination = pathlib.Path(destination)
  destination.parent.mkdir(parents=True, exist_ok=True)
  with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
    root = pathlib.Path(temporary)
    stage = root / 'windows'
    bottles.bundle(stage, root / 'packages', [('wimlib', 'wimlib-imagex'), ('cdrtools', 'mkisofs')])
    if package is None:
      package = root / 'drivers.rpm'
      download(package)
    drivers(pathlib.Path(package), stage)
    subprocess.check_call([str(stage / 'bin/wimlib-imagex'), '--version'])
    subprocess.check_call([str(stage / 'bin/mkisofs'), '--version'])
    manifest.write(stage)
    previous = root / 'previous'
    if destination.exists():
      destination.rename(previous)
    try:
      stage.rename(destination)
    except BaseException:
      if previous.exists():
        previous.rename(destination)
      raise
    print(f'[windows] -> {destination}')


if __name__ == '__main__':
  main(sys.argv[1], sys.argv[2] if len(sys.argv) == 3 else None)
