"""verify the host-owned native snapshot controller on a real signed worker."""

import hashlib
import json
import os
import pathlib
import platform
import shutil
import struct
import subprocess
import tempfile
import uuid


def main(registry=False, automatic=False, configured=False, agent=False, paused=False):
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        raise SystemExit('Native snapshot checks require Apple silicon macOS')
    root = pathlib.Path(__file__).resolve().parents[2]
    subprocess.run(['cargo', 'build', '-p', 'machine', '--bin', 'hoppervm', '-p', 'engine',
                    '--example', 'snapshotcontroller', '--locked'], cwd=root,
                   env=dict(os.environ, CARGO_INCREMENTAL='0'), check=True)
    environment = dict(os.environ, HOPPER_RESTORE_REGISTRY="1" if registry else "0",
                       HOPPER_RESTORE_AUTOMATIC="1" if automatic else "0",
                       HOPPER_RESTORE_CONFIGURED="1" if configured else "0",
                       HOPPER_RESTORE_PAUSED="1" if paused else "0")
    if paused and not (registry and automatic and configured):
        raise ValueError('Paused restore requires the configured automatic registry profile')
    environment.pop('HOPPER_RESTORE_MCP', None)
    if agent:
        if not registry or not automatic:
            raise ValueError('MCP restore requires the automatic registry profile')
        subprocess.run(['cargo', 'build', '-p', 'mcp', '--example', 'guestagent', '--locked'],
                       cwd=root, env=dict(os.environ, CARGO_INCREMENTAL='0'), check=True)
        environment['HOPPER_RESTORE_MCP'] = str(root / 'target/debug/examples/guestagent')
    fixture = pathlib.Path(tempfile.mkdtemp(prefix='hoppersnapshot.'))
    try:
        executable = root / 'target/debug/examples/snapshotcontroller'
        worker = fixture / 'hoppervm'
        if configured:
            if not registry or not automatic:
                raise ValueError('Configured restore requires the automatic registry profile')
            contents = fixture / 'Hopper Probe.app/Contents'
            worker = contents / 'MacOS/sidecars/hoppervm'
            worker.parent.mkdir(parents=True)
            executable = contents / 'MacOS/snapshotcontroller'
            shutil.copy2(root / 'target/debug/examples/snapshotcontroller', executable)
        shutil.copy2(root / 'target/debug/hoppervm', worker)
        subprocess.run(['codesign', '--force', '--sign', '-', '--entitlements',
                        str(root / 'assets/machine.entitlements'), str(worker)], check=True)
        subprocess.run(['codesign', '--verify', '--strict', str(worker)], check=True)
        firmware = struct.pack('<I', 0x14000000)
        if automatic:
            # Continuously change guest RAM so paused handoff checks can detect CPU execution.
            firmware = struct.pack('<IIIIIIQ', 0x580000C0, 0xB9400001, 0x11000421,
                                   0xB9000001, 0x17FFFFFD, 0xD503201F, 0x40100000)
        if registry and not automatic:
            # First execution writes guest RAM, then loops. Held cold capture must precede it.
            firmware = struct.pack('<IIIIQ', 0x58000080, 0x52800A21,
                                   0xB9000001, 0x14000000, 0x40100000)
        (fixture / 'firmware').write_bytes(firmware)
        (fixture / 'template').write_bytes(b'\xff')
        if configured:
            folder = contents / 'Resources/firmware'
            folder.mkdir(parents=True)
            (folder / 'windows.fd').write_bytes(firmware)
            (folder / 'variables.fd').write_bytes(b'\xff')
            (folder / 'manifest.json').write_text(json.dumps(dict(
                size=len(firmware), sha256=hashlib.sha256(firmware).hexdigest(),
                variables=dict(size=1, sha256=hashlib.sha256(b'\xff').hexdigest()))))
        identity = str(uuid.uuid4())
        records = fixture / 'machines/records'
        records.mkdir(parents=True)
        (records / f'{identity}.json').write_text(json.dumps(dict(
            id=identity, name='Synthetic controller probe', guest='windows', profile='windows',
            resources=dict(cpus=2, memoryGib=1 if configured else 2, diskGib=64), agentAccess=True,
            agentGeneration=0, runtime='hypervisor', installer=None)))
        subprocess.run([str(executable),
                        str(fixture), identity, str(worker)], check=True,
                       timeout=1800 if configured else 120 if automatic else 60,
                       env=environment)
    except BaseException:
        print(f'Failed snapshot controller fixture preserved: {fixture}', flush=True)
        raise
    else:
        shutil.rmtree(fixture)


if __name__ == '__main__':
    main()
    main(registry=True)
    main(registry=True, automatic=True)
    main(registry=True, automatic=True, agent=True)
    main(registry=True, automatic=True, configured=True)
