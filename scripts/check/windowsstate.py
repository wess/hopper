"""recover an installed Windows disposable fixture from a complete on-disk checkpoint."""

import argparse
import fcntl
import hashlib
import json
import os
import pathlib
import platform
import shutil
import stat
import subprocess
import tempfile
import time


def stamp(path):
  metadata = path.stat()
  return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns)


def main():
  parser = argparse.ArgumentParser()
  parser.add_argument('source', type=pathlib.Path)
  parser.add_argument('firmware', type=pathlib.Path)
  parser.add_argument('identity')
  args = parser.parse_args()
  if platform.system() != 'Darwin' or platform.machine() != 'arm64':
    raise SystemExit('Windows checkpoint checks require Apple silicon macOS')
  source = args.source.resolve(strict=True)
  metadata = source.stat()
  if (source.parent != pathlib.Path('/private/tmp')
      or not source.name.startswith('hopperrecovery119.')
      or stat.S_IMODE(metadata.st_mode) != 0o700 or metadata.st_uid != os.geteuid()):
    raise ValueError('Use an existing private disposable recovery fixture')
  if len(args.identity.encode()) != 20:
    raise ValueError('Expected the fixture disk identity')
  if shutil.disk_usage(source).free < 8 * 1024**3:
    raise ValueError('Windows checkpoint verification requires 8 GiB free space')
  firmware = args.firmware.resolve(strict=True)
  root = pathlib.Path(__file__).resolve().parents[2]
  environment = dict(os.environ, CARGO_INCREMENTAL='0')
  subprocess.run(['cargo', 'build', '-p', 'machine', '--example', 'desktoprestore', '--locked'],
                 cwd=root, env=environment, check=True)
  fixture = pathlib.Path(tempfile.mkdtemp(prefix='hopperrecovery119.process.', dir='/tmp'))
  fixture.chmod(0o700)
  print(f'Windows process-recovery fixture: {fixture}', flush=True)
  before = {name: stamp(source / name) for name in ['disk', 'variables/bank', 'variables/journal']}
  locks = []
  try:
    for name in ['disk', 'variables/lock']:
      descriptor = os.open(source / name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
      locks.append(descriptor)
      if not stat.S_ISREG(os.fstat(descriptor).st_mode):
        raise ValueError('Fixture lock source is not regular')
      fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    (fixture / 'variables').mkdir(mode=0o700)
    for name in before:
      subprocess.run(['cp', '-c', str(source / name), str(fixture / name)], check=True)
      (fixture / name).chmod(0o600)
    if any(stamp(source / name) != value for name, value in before.items()):
      raise ValueError('Source fixture changed during cloning')
  finally:
    for descriptor in locks:
      os.close(descriptor)
  binary = fixture / 'desktoprestore'
  shutil.copy2(root / 'target/debug/examples/desktoprestore', binary)
  subprocess.run(['codesign', '--force', '--sign', '-', '--entitlements',
                  str(root / 'assets/machine.entitlements'), str(binary)], check=True)
  subprocess.run(['codesign', '--verify', '--strict', str(binary)], check=True)
  timings = {}
  checkpoint_before = None
  for mode in ['save', 'load']:
    started = time.monotonic()
    with (fixture / f'{mode}.log').open('x') as log:
      subprocess.run([str(binary), str(firmware), str(fixture), args.identity, mode],
                     stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)
    timings[mode] = time.monotonic() - started
    package = fixture / 'snapshots' / ('02' * 16)
    current = {str(path.relative_to(package)): stamp(path)
               for path in package.rglob('*') if path.is_file()}
    if mode == 'save':
      checkpoint_before = current
    elif current != checkpoint_before:
      raise ValueError('Fresh-process recovery changed snapshot inode/size/mtime')
    print(f'Windows fresh-process {mode} passed in {timings[mode]:.2f}s', flush=True)
  if (fixture / 'boot.ppm').read_bytes() != (fixture / 'held.ppm').read_bytes():
    raise ValueError('Fresh-process held Windows display differs from its source')
  if any(stamp(source / name) != value for name, value in before.items()):
    raise ValueError('Source fixture changed during recovery')
  receipt = {
    'source': str(source), 'fixture': str(fixture), 'timings': timings,
    'held_pixels_match': True, 'source_unchanged': True,
    'snapshot_metadata_unchanged': True,
    'manifest_sha256': hashlib.sha256(
      (fixture / 'snapshots' / ('02' * 16) / 'manifest.hsm').read_bytes()).hexdigest(),
    'scope': 'installed Windows fresh-process recovery; inspect recovered frame for input readiness',
  }
  (fixture / 'evidence.json').write_text(json.dumps(receipt, indent=2) + '\n')
  (fixture / 'evidence.json').chmod(0o600)
  print(f'Windows fresh-process recovery passed; inspect {fixture / "recovered.ppm"}', flush=True)


if __name__ == '__main__':
  main()
