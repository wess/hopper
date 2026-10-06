#!/usr/bin/env python3
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def dependencies(path):
    lines = run('otool', '-L', str(path)).splitlines()[1:]
    return [line.strip().split(' (compatibility')[0] for line in lines]


def main():
    destination = pathlib.Path(sys.argv[1])
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        stage = pathlib.Path(temporary) / 'runtime'
        for folder in ['bin', 'lib', 'share/qemu', 'share/licenses']:
            (stage / folder).mkdir(parents=True)
        packages = pathlib.Path(temporary) / 'packages'
        packages.mkdir()
        roots = {}
        def package(formula):
            if formula in roots:
                return roots[formula]
            bottle = run('brew', '--cache', '--force-bottle', formula)
            unpacked = packages / formula
            unpacked.mkdir()
            subprocess.check_call(['tar', '-xzf', bottle, '-C', str(unpacked)])
            versions = [p for p in (unpacked / formula).iterdir() if p.is_dir()]
            if len(versions) != 1:
                raise RuntimeError(f'Unexpected bottle layout for {formula}')
            roots[formula] = versions[0]
            return versions[0]

        def resolve(dependency, source):
            match = re.match(r'(?:@@HOMEBREW_PREFIX@@|/opt/homebrew|/usr/local)/opt/([^/]+)/(.+)', dependency)
            if match:
                return package(match.group(1)) / match.group(2)
            match = re.match(r'(?:@@HOMEBREW_CELLAR@@|/opt/homebrew/Cellar|/usr/local/Cellar)/([^/]+)/[^/]+/(.+)', dependency)
            if match:
                return package(match.group(1)) / match.group(2)
            if dependency.startswith('@loader_path/'):
                return source.parent / dependency.removeprefix('@loader_path/')
            raise RuntimeError(f'Unresolved library {dependency} in {source}')

        formulas = set()
        copied = {}

        def copy(source, relative):
            source = pathlib.Path(source).resolve()
            target = stage / relative
            if target in copied:
                if copied[target] != source:
                    raise RuntimeError(f'Conflicting library: {target.name}')
                return target
            copied[target] = source
            shutil.copyfile(source, target)
            target.chmod(0o755)
            match = re.search(re.escape(str(packages)) + r'/([^/]+)/', str(source))
            if match:
                formulas.add(match.group(1))
            changes = []
            for dependency in dependencies(source):
                if dependency.startswith(('/System/', '/usr/lib/')):
                    continue
                dependency_path = resolve(dependency, source)
                if dependency_path.resolve() == source:
                    continue
                copy(dependency_path, pathlib.Path('lib') / pathlib.Path(dependency).name)
                replacement = ('@loader_path/' if relative.parts[0] == 'lib' else '@loader_path/../lib/') + pathlib.Path(dependency).name
                changes.extend(['-change', dependency, replacement])
            if relative.parts[0] == 'lib':
                changes.extend(['-id', '@loader_path/' + target.name])
            if changes:
                subprocess.check_call(['install_name_tool', *changes, str(target)])
            return target

        for formula, executable in [('qemu', 'qemu-system-aarch64'), ('qemu', 'qemu-img'), ('swtpm', 'swtpm')]:
            prefix = package(formula)
            copy(prefix / 'bin' / executable, pathlib.Path('bin') / executable)
        firmware = package('qemu') / 'share/qemu'
        shutil.copytree(firmware, stage / 'share/qemu', dirs_exist_ok=True)
        manifest = json.loads(run('brew', 'info', '--json=v2', *sorted(formulas)))
        (stage / 'share/licenses/packages.json').write_text(json.dumps(manifest, indent=2) + '\n')
        for formula in sorted(formulas):
            prefix = package(formula).resolve()
            license_dir = stage / 'share/licenses' / formula
            license_dir.mkdir()
            for path in prefix.iterdir():
                if path.is_file() and re.match(r'^(COPYING|LICENSE|NOTICE|AUTHORS)', path.name, re.I):
                    shutil.copyfile(path, license_dir / path.name)
        for target in copied:
            subprocess.check_call(['codesign', '--force', '--sign', '-', str(target)])
        entitlement = pathlib.Path(__file__).resolve().parents[2] / 'assets/qemu.entitlements'
        subprocess.check_call(['codesign', '--force', '--entitlements', str(entitlement), '--sign', '-', str(stage / 'bin/qemu-system-aarch64')])
        run(str(stage / 'bin/qemu-system-aarch64'), '--version')
        run(str(stage / 'bin/qemu-img'), '--version')
        run(str(stage / 'bin/swtpm'), '--version')
        subprocess.check_call([sys.executable, str(pathlib.Path(__file__).with_name('probe.py')), str(stage)])
        if destination.exists():
            shutil.rmtree(destination)
        shutil.move(str(stage), destination)
        print(f'[qemu] -> {destination}')


if __name__ == '__main__':
    main()
