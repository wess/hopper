import hashlib
import json
import pathlib
import sys

URL = ('https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/archive-virtio/'
       'virtio-win-0.1.302-1/virtio-win-0.1.302-1.noarch.rpm')
SHA256 = '2f58eea024e0221a873032761dc3ed3262409cd80860b7b27d3d5a0f206616c9'
DRIVERS = {
  'viostor': ('viostor.inf', 'viostor.cat', 'viostor.sys'),
  'vioscsi': ('vioscsi.inf', 'vioscsi.cat', 'vioscsi.sys'),
  'vioinput': ('vioinput.inf', 'vioinput.cat', 'vioinput.sys', 'viohidkmdf.sys'),
  'vioserial': ('vioser.inf', 'vioser.cat', 'vioser.sys'),
}


def digest(path):
  value = hashlib.sha256()
  with path.open('rb') as source:
    for chunk in iter(lambda: source.read(1024 * 1024), b''):
      value.update(chunk)
  return value.hexdigest()


def contents(root):
  root = pathlib.Path(root)
  files = {}
  for path in sorted(root.rglob('*')):
    if path.is_symlink():
      raise ValueError('Windows assets cannot contain symlinks')
    if not path.is_file() or path == root / 'manifest.json':
      continue
    name = path.relative_to(root).as_posix()
    size = path.stat().st_size
    if not 1 <= size <= 64 * 1024 * 1024:
      raise ValueError(f'Invalid Windows asset size: {name}')
    files[name] = {
      'size': size, 'sha256': digest(path), 'executable': name.startswith(('bin/', 'lib/')),
    }
  if not 1 <= len(files) <= 256:
    raise ValueError('Invalid Windows asset count')
  required = {'bin/wimlib-imagex', 'bin/mkisofs', 'licenses/virtio.txt', 'licenses/packages.json'}
  required.update(f'drivers/{group}/w11/ARM64/{name}'
                  for group, names in DRIVERS.items() for name in names)
  if not required <= files.keys():
    raise ValueError('Incomplete Windows asset package')
  return {'version': 1, 'driverPackage': {'url': URL, 'sha256': SHA256}, 'files': files}


def write(root):
  root = pathlib.Path(root)
  manifest = contents(root)
  (root / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
  if sys.argv[1] == '--verify':
    root = pathlib.Path(sys.argv[2])
    if json.loads((root / 'manifest.json').read_text()) != contents(root):
      raise ValueError('Windows assets manifest mismatch')
  else:
    write(sys.argv[1])
