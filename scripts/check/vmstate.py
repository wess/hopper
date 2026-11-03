"""verify complete native checkpoint recovery after the source process exits."""

import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile


def inventory(directory):
  result = {}
  for path in sorted(directory.rglob('*')):
    if path.is_file():
      digest = hashlib.sha256()
      with path.open('rb') as file:
        while chunk := file.read(65536):
          digest.update(chunk)
      metadata = path.stat()
      result[str(path.relative_to(directory))] = (
        metadata.st_ino, metadata.st_size, metadata.st_mtime_ns,
        metadata.st_mode, metadata.st_nlink, digest.digest(),
      )
  return result


def main():
  if platform.system() != 'Darwin' or platform.machine() != 'arm64':
    raise SystemExit('Native VM recovery checks require Apple silicon macOS')
  root = pathlib.Path(__file__).resolve().parents[2]
  result = subprocess.run(
    ['cargo', 'build', '-p', 'machine', '--example', 'group', '--locked',
     '--message-format=json'], cwd=root, env=dict(os.environ, CARGO_INCREMENTAL='0'),
    check=False, stdout=subprocess.PIPE, text=True,
  )
  if result.returncode:
    for line in result.stdout.splitlines():
      item = json.loads(line)
      if item.get('reason') == 'compiler-message':
        print(item.get('message', {}).get('rendered', ''), end='', file=sys.stderr)
    result.check_returncode()
  binaries = []
  for line in result.stdout.splitlines():
    item = json.loads(line)
    if (item.get('reason') == 'compiler-artifact'
        and item.get('target', {}).get('name') == 'group' and item.get('executable')):
      binaries.append(item['executable'])
  if len(binaries) != 1:
    raise ValueError('Expected one group diagnostic executable')
  temporary = tempfile.mkdtemp(prefix='hoppervmstate.')
  try:
    fixture = pathlib.Path(temporary)
    fixture.chmod(0o700)
    binary = fixture / 'group'
    shutil.copy2(binaries[0], binary)
    subprocess.run(['codesign', '--force', '--sign', '-', '--entitlements',
                    str(root / 'assets/machine.entitlements'), str(binary)], check=True)
    subprocess.run(['codesign', '--verify', '--strict', str(binary)], check=True)
    subprocess.run([str(binary)], check=True, timeout=90)
    print('persistent capture, firmware durability and owner lifecycle checks passed', flush=True)
    parent = fixture / 'snapshots'
    parent.mkdir(mode=0o700)
    # each subprocess has exited before any later process opens the package.
    subprocess.run([str(binary), 'save', str(parent)], check=True, timeout=90)
    package = parent / ('02' * 16)
    original = inventory(package)
    for attempt in range(2):
      restored = subprocess.run([str(binary), 'load', str(parent)],
                                check=False, timeout=90, capture_output=True, text=True)
      print(restored.stdout, end='', flush=True)
      if restored.returncode:
        print(restored.stderr, end='', flush=True)
        restored.check_returncode()
      if restored.stdout.count('Durable restore attempt rejects replay') != 1:
        raise ValueError('Missing durable restore-attempt rejection evidence')
      if restored.stdout.count('Consumed restore generation rejects replay after complete') != 3:
        raise ValueError('Missing post-teardown restore replay evidence')
      if restored.stdout.count('Bound restore generation published, original owners dropped') != 3:
        raise ValueError('Missing published/reopened restore-generation evidence')
      if restored.stdout.count('Restore generation rejects revoked preparation') != 1:
        raise ValueError('Missing restore-generation publication failure evidence')
      if restored.stdout.count('Active generation cold boot retains mutable disk/EFI') != 3:
        raise ValueError('Missing mutable-generation fresh-boot evidence')
      if restored.stdout.count('Persistent restored runtime recaptures durable disk/EFI state') != 1:
        raise ValueError('Missing persistent restored-runtime recapture evidence')
      counts = [int(line.split(': ')[1]) for line in restored.stdout.splitlines()
                if line.startswith('Checkpoint authorization checks: ')]
      if len(counts) != 1 or counts[0] < 3:
        raise ValueError('Missing checkpoint authorization evidence')
      authorization_count = counts[0]
      if inventory(package) != original:
        raise ValueError('Fresh-process recovery mutated the saved checkpoint')
      print(f'fresh-process recovery {attempt + 1} passed', flush=True)

    def reject(mode, expected, environment=None, destination=parent):
      result = subprocess.run([str(binary), mode, str(destination)],
                              text=True, capture_output=True, timeout=90, env=environment)
      expected = (expected,) if isinstance(expected, str) else expected
      if result.returncode == 0 or not any(message in result.stderr for message in expected):
        raise ValueError(f'Wrong rejection: {result.stderr[:512]}')

    for deny in [0, 1, 2, authorization_count - 1]:
      reject('load', 'Snapshot authorization revoked',
             dict(os.environ, HOPPER_VMSTATE_DENY=str(deny)))
    for deny in [0, 2, 200]:
      destination = fixture / f'denied{deny}'
      destination.mkdir(mode=0o700)
      reject('save', ('Snapshot authorization revoked', 'Snapshot publication cancelled'),
             dict(os.environ, HOPPER_VMSTATE_SAVE_DENY=str(deny)), destination)
      if (destination / ('02' * 16)).exists():
        raise ValueError('Revoked background writer published a checkpoint')
      if deny == 200:
        stages = list(destination.glob('.stage*'))
        if (len(stages) != 1 or not (stages[0] / 'storage0/image').exists()
            or (stages[0] / 'backend0.hvb').exists()):
          raise ValueError('Revocation did not interrupt staged-image verification')
    reject('save', 'Publish snapshot without replacement')
    if inventory(package) != original:
      raise ValueError('Duplicate publication changed the original package')
    cpu = package / '0.hcp'
    saved = cpu.read_bytes()
    replacement = (package / '1.hcp').read_bytes()
    if len(saved) != len(replacement):
      raise ValueError('CPU substitution fixture sizes differ')
    cpu.chmod(0o600)
    cpu.write_bytes(replacement)
    cpu.chmod(0o400)
    reject('load', 'does not match its manifest')
    cpu.chmod(0o600)
    damaged = bytearray(saved)
    damaged[100] ^= 1
    cpu.write_bytes(damaged)
    cpu.chmod(0o400)
    reject('load', 'checksum')
    print('background staging, owner receipt commit, writer revocation, complete VM fresh-process recovery, independent recapture, unchanged source, '
          'no replacement and CPU substitution/damage rejection passed', flush=True)

  except BaseException:
    print(f'Failed recovery fixture preserved: {temporary}', flush=True)
    raise
  else:
    shutil.rmtree(temporary)


if __name__ == '__main__':
  main()
