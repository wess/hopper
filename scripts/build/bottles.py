import json
import pathlib
import re
import shutil
import subprocess


def run(*args):
  return subprocess.check_output(args, text=True).strip()


def bundle(stage, packages, executables):
  roots, copied, formulas = {}, {}, set()
  for folder in ('bin', 'lib', 'licenses'):
    (stage / folder).mkdir(parents=True)

  def package(formula):
    if formula not in roots:
      bottle = run('brew', '--cache', '--force-bottle', formula)
      unpacked = packages / formula
      unpacked.mkdir(parents=True)
      subprocess.check_call(['/usr/bin/tar', '-xzf', bottle, '-C', str(unpacked)])
      versions = [p for p in (unpacked / formula).iterdir() if p.is_dir()]
      if len(versions) != 1:
        raise ValueError(f'Unexpected bottle layout for {formula}')
      roots[formula] = versions[0]
    return roots[formula]

  def resolve(dependency, source):
    match = re.fullmatch(
      r'(?:@@HOMEBREW_PREFIX@@|/opt/homebrew|/usr/local)/opt/([^/]+)/(.+)', dependency,
    )
    if match:
      return package(match[1]) / match[2]
    match = re.fullmatch(
      r'(?:@@HOMEBREW_CELLAR@@|/opt/homebrew/Cellar|/usr/local/Cellar)/([^/]+)/[^/]+/(.+)',
      dependency,
    )
    if match:
      return package(match[1]) / match[2]
    if dependency.startswith('@loader_path/'):
      return source.parent / dependency.removeprefix('@loader_path/')
    raise ValueError(f'Unresolved library {dependency} in {source}')

  def copy(source, relative):
    source = source.resolve()
    target = stage / relative
    if target in copied:
      if copied[target] != source:
        raise ValueError(f'Conflicting library {target.name}')
      return
    if not source.is_file() or source.stat().st_size > 64 * 1024 * 1024:
      raise ValueError(f'Invalid media tool input {source}')
    copied[target] = source
    shutil.copyfile(source, target)
    target.chmod(0o755)
    match = re.search(re.escape(str(packages)) + r'/([^/]+)/', str(source))
    if match:
      formulas.add(match[1])
    changes = []
    for line in run('otool', '-L', str(source)).splitlines()[1:]:
      dependency = line.strip().split(' (compatibility')[0]
      if dependency.startswith(('/System/', '/usr/lib/')):
        continue
      original = resolve(dependency, source)
      if original.resolve() == source:
        continue
      name = pathlib.Path(dependency).name
      copy(original, pathlib.Path('lib') / name)
      prefix = '@loader_path/' if relative.parts[0] == 'lib' else '@loader_path/../lib/'
      changes.extend(['-change', dependency, prefix + name])
    if relative.parts[0] == 'lib':
      changes.extend(['-id', '@loader_path/' + target.name])
    if changes:
      subprocess.check_call(['install_name_tool', *changes, str(target)])
    if 'arm64' not in run('lipo', '-archs', str(target)).split():
      raise ValueError(f'Media tool is not ARM64: {target}')

  for formula, executable in executables:
    copy(package(formula) / 'bin' / executable, pathlib.Path('bin') / executable)
  metadata = json.loads(run('brew', 'info', '--json=v2', *sorted(formulas)))
  (stage / 'licenses/packages.json').write_text(json.dumps(metadata, indent=2) + '\n')
  for formula in sorted(formulas):
    folder = stage / 'licenses' / formula
    folder.mkdir()
    for path in package(formula).iterdir():
      if path.is_file() and re.match(r'^(COPYING|LICENSE|NOTICE|AUTHORS)', path.name, re.I):
        shutil.copyfile(path, folder / path.name)
  for target in copied:
    subprocess.check_call(['codesign', '--force', '--sign', '-', str(target)])
