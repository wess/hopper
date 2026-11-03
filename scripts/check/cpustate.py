"""verify architectural CPU records across signed processes and unsafe file inputs."""

import json
import os
import pathlib
import platform
import subprocess
import tempfile


def write(path, data):
  descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
  with os.fdopen(descriptor, 'wb') as file:
    file.write(data)


def main(example='cpustate', filename='cpu.hcs', limit=4096):
  if platform.system() != 'Darwin' or platform.machine() != 'arm64':
    raise SystemExit('Native record checks require Apple silicon macOS')
  root = pathlib.Path(__file__).resolve().parents[2]
  result = subprocess.run(
    ['cargo', 'build', '-p', 'machine', '--example', example, '--locked',
     '--message-format=json'],
    cwd=root, env=dict(os.environ, CARGO_INCREMENTAL='0'), check=True,
    stdout=subprocess.PIPE, text=True,
  )
  binaries = []
  for line in result.stdout.splitlines():
    item = json.loads(line)
    if (item.get('reason') == 'compiler-artifact'
        and item.get('target', {}).get('name') == example
        and item.get('executable')):
      binaries.append(item['executable'])
  if len(binaries) != 1:
    raise ValueError(f'Expected one {example} diagnostic executable')
  binary = binaries[0]
  subprocess.run(
    ['codesign', '--force', '--sign', '-', '--entitlements',
     str(root / 'assets/machine.entitlements'), binary], check=True,
  )
  subprocess.run(['codesign', '--verify', '--strict', binary], check=True)
  with tempfile.TemporaryDirectory(prefix=f'hopper{example}.') as temporary:
    fixture = pathlib.Path(temporary)
    fixture.chmod(0o700)
    subprocess.run([binary, '--save', str(fixture)], check=True, timeout=30)
    saved = fixture / filename
    original = saved.read_bytes()
    metadata = saved.stat()
    subprocess.run([binary, '--restore', str(fixture)], check=True, timeout=30)

    def reject(directory, expected, stage='--restore'):
      result = subprocess.run(
        [binary, stage, str(directory)], text=True, capture_output=True, timeout=30,
      )
      if result.returncode == 0 or expected not in result.stderr:
        raise ValueError(f'Wrong rejection: {result.stderr[:512]}')

    reject(fixture, 'File exists', '--save')
    for name in ['symlink', 'hardlink', 'public', 'fifo', 'directory', 'oversized', 'damaged']:
      directory = fixture / name
      directory.mkdir(mode=0o700)
      file = directory / filename
      if name == 'symlink':
        file.symlink_to(saved)
        reject(directory, 'Too many levels of symbolic links')
      elif name == 'hardlink':
        os.link(saved, file)
        reject(directory, 'not a private bounded file')
        file.unlink()
      elif name == 'fifo':
        os.mkfifo(file, 0o600)
        reject(directory, 'not a private bounded file')
      elif name == 'directory':
        file.mkdir(mode=0o700)
        reject(directory, 'not a private bounded file')
      else:
        data = bytearray(original) if name != 'oversized' else bytes(limit + 1)
        if name == 'damaged':
          data[150] ^= 1
        write(file, data)
        if name == 'public':
          file.chmod(0o644)
        reject(directory, 'checksum is invalid' if name == 'damaged' else 'not a private bounded file')
    fixture.chmod(0o755)
    reject(fixture, 'directory is not private')
    fixture.chmod(0o700)
    current = saved.stat()
    if (saved.read_bytes() != original or current.st_ino != metadata.st_ino
        or current.st_mtime_ns != metadata.st_mtime_ns or current.st_nlink != 1):
      raise ValueError('A rejection changed the original record')
    print(f'{example}: fresh-process restore and private-file/checksum/overwrite rejection checks passed')


if __name__ == '__main__':
  main()
