import html
import hashlib
import json
import pathlib
import re
import subprocess
import sys


TARGET = 'aarch64-unknown-linux-musl'
MUSL_NOTICE = pathlib.Path(__file__).resolve().parents[2] / 'crates/guest/licenses/musl.txt'
MUSL_SHA256 = 'f9bc4423732350eb0b3f7ed7e91d530298476f8fec0c6c427a1c04ade22655af'


def musl_notice():
  if MUSL_NOTICE.is_symlink() or not MUSL_NOTICE.is_file():
    raise ValueError('Missing reviewed musl notice')
  data = MUSL_NOTICE.read_bytes()
  if len(data) > 32768 or hashlib.sha256(data).hexdigest() != MUSL_SHA256:
    raise ValueError('Reviewed musl notice changed')
  return '<h2>musl 1.2.5</h2><pre>' + html.escape(data.decode()) + '</pre>'


def cargo(*args):
  return subprocess.check_output(['cargo', *args], text=True)


def notices():
  metadata = json.loads(cargo('metadata', '--format-version', '1', '--filter-platform', TARGET,
                              '--locked'))
  tree = cargo('tree', '-p', 'guest', '--target', TARGET, '--edges', 'normal,build',
               '--locked', '--prefix', 'none', '--format', '{p}')
  selected = set()
  for line in tree.splitlines():
    match = re.match(r'^(\S+) v(\S+)(?: |$)', line)
    if not match:
      raise ValueError('Unrecognized guest dependency')
    selected.add(match.groups())
  output = ['<!DOCTYPE html><html><head><meta charset="utf-8">',
            '<title>Hopper guest helper notices</title></head><body>',
            '<h1>Hopper guest helper dependency notices</h1>',
            '<p>Includes normal and build dependencies for ' + TARGET + '.</p>']
  found = set()
  for package in sorted(metadata['packages'], key=lambda item: (item['name'], item['version'])):
    identity = package['name'], package['version']
    if identity not in selected:
      continue
    if identity in found:
      raise ValueError('Ambiguous guest dependency')
    found.add(identity)
    title = ' '.join(identity)
    output.append('<h2>' + html.escape(title) + '</h2>')
    output.append('<p>Declared license: ' + html.escape(package['license'] or '') + '</p>')
    if package['source'] is None:
      if package['license'] != 'MIT':
        raise ValueError('Unexpected project license: ' + title)
      path = pathlib.Path(metadata['workspace_root']) / 'LICENSE'
      if path.is_symlink() or not path.is_file() or not 1 <= path.stat().st_size <= 4096:
        raise ValueError('Missing project license text')
      text = path.read_text()
      if '\0' in text:
        raise ValueError('Invalid project license text')
      output.append('<pre>' + html.escape(text) + '</pre>')
      continue
    root = pathlib.Path(package['manifest_path']).parent
    files = sorted(path for path in root.iterdir()
                   if path.name.upper().startswith(('LICENSE', 'COPYING', 'NOTICE')))
    if not files:
      raise ValueError('Missing dependency license: ' + title)
    if 'Unicode-3.0' in (package['license'] or '') and not any(
        path.name == 'LICENSE-UNICODE' for path in files):
      raise ValueError('Missing required Unicode license: ' + title)
    for path in files:
      if path.is_symlink() or not path.is_file() or not 1 <= path.stat().st_size <= 128 * 1024:
        raise ValueError('Invalid dependency notice: ' + str(path))
      text = path.read_text()
      if '\0' in text:
        raise ValueError('Invalid dependency notice text')
      output.extend(['<h3>' + html.escape(path.name) + '</h3>',
                     '<pre>' + html.escape(text) + '</pre>'])
  if found != selected:
    raise ValueError('Incomplete guest dependency inventory')
  sysroot = pathlib.Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
  standard = sysroot / 'share/doc/rust/COPYRIGHT-library.html'
  if standard.is_symlink() or not standard.is_file() or standard.stat().st_size > 2 * 1024 * 1024:
    raise ValueError('Install rust-docs for guest standard-library notices')
  document = standard.read_text()
  body = re.search(r'<body>(.*)</body>', document, re.DOTALL)
  if not body:
    raise ValueError('Invalid standard-library notices')
  version = subprocess.check_output(['rustc', '--version'], text=True).strip()
  if not version.startswith('rustc 1.99.0 '):
    raise ValueError('Review musl linkage and notices for this guest toolchain; expected Rust 1.99.0')
  output.extend(['<h2>Toolchain: ' + html.escape(version) + '</h2>', body.group(1), musl_notice(), '</body></html>'])
  result = '\n'.join(output).encode()
  if len(result) > 4 * 1024 * 1024:
    raise ValueError('Guest notices exceed resource bounds')
  return result


if __name__ == '__main__':
  if len(sys.argv) != 2:
    raise SystemExit('usage: guestlicenses.py OUTPUT')
  path = pathlib.Path(sys.argv[1])
  if path.exists() or path.is_symlink():
    raise ValueError('Guest notice output must be new')
  with path.open('xb') as file:
    file.write(notices())
