"""verify native worker identity and lifecycle on real signed hardware."""

import json
import os
import pathlib
import platform
import select
import shutil
import struct
import subprocess
import tempfile
import time
import uuid

import workersnapshot


def receive(pipe):
    deadline = time.monotonic() + 10

    def exact(length):
        result = bytearray()
        while len(result) < length:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([pipe], [], [], remaining)[0]:
                raise TimeoutError('Native worker reply deadline expired')
            chunk = os.read(pipe.fileno(), length - len(result))
            if not chunk:
                raise EOFError('Native worker disconnected')
            result.extend(chunk)
        return result

    prefix = exact(8)
    length = struct.unpack('<I', prefix[4:])[0]
    if prefix[:4] != b'HPV1' or not 0 < length <= 16384:
        raise ValueError('Invalid native worker header')
    return json.loads(exact(length))


def exchange(child, identity, command):
    encoded = json.dumps({'id': identity, 'command': command}).encode()
    child.stdin.write(b'HPV1' + struct.pack('<I', len(encoded)) + encoded)
    child.stdin.flush()
    reply = receive(child.stdout)
    if reply['id'] != identity:
        raise ValueError('Native worker reply identity differs')
    return reply['result']


def main():
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        raise SystemExit('Native worker checks require Apple silicon macOS')
    root = pathlib.Path(__file__).resolve().parents[2]
    subprocess.run(['cargo', 'build', '-p', 'machine', '--bin', 'hoppervm', '-p', 'engine',
                    '--example', 'snapshotcommit', '--locked'],
                   cwd=root, env=dict(os.environ, CARGO_INCREMENTAL='0'), check=True)
    fixture = pathlib.Path(tempfile.mkdtemp(prefix='hopperworker.'))
    child = None
    try:
        binary = fixture / 'hoppervm'
        shutil.copy2(root / 'target/debug/hoppervm', binary)
        subprocess.run(['codesign', '--force', '--sign', '-', '--entitlements',
                        str(root / 'assets/machine.entitlements'), str(binary)], check=True)
        subprocess.run(['codesign', '--verify', '--strict', str(binary)], check=True)
        (fixture / 'firmware').write_bytes(struct.pack('<I', 0x14000000))
        (fixture / 'template').write_bytes(b'\xff')
        machine = str(uuid.uuid4())
        boot = dict(machineId=machine, firmware=str(fixture / 'firmware'),
                    variables=str(fixture / 'template'), store=str(fixture / 'variables'),
                    bootMedia=None, installer=None, disk=None, diskId='hopper00000000000001',
                    memory=268435456, cpus=2, timeoutMs=10000)
        fingerprints = []
        handoff = None
        with (fixture / 'worker.log').open('wb') as log:
            for attempt in range(4):
                boot['cpus'] = 1 if attempt == 2 else 2
                boot['timeoutMs'] = 30000 if attempt == 0 else 10000
                if attempt == 3:
                    (fixture / 'firmware').write_bytes(struct.pack('<II', 0x14000000, 0xd503201f))
                child = subprocess.Popen([str(binary)], stdin=subprocess.PIPE,
                                         stdout=subprocess.PIPE, stderr=log, bufsize=0)
                reply = exchange(child, 1, dict(type='start', boot=boot))
                configuration = reply.get('configuration')
                if (reply.get('type') != 'started' or reply.get('machineId') != machine
                        or not isinstance(configuration, list) or len(configuration) != 32
                        or not all(type(byte) is int and 0 <= byte <= 255 for byte in configuration)
                        or not any(configuration)):
                    raise ValueError(f'Wrong native startup identity/configuration: {reply}')
                fingerprints.append(configuration)
                commands = [(2, 'status', 'status'), (3, 'pause', 'paused'),
                            (4, 'resume', 'running')]
                if attempt != 0:
                    commands.append((5, 'stop', 'stopped'))
                for identity, command, expected in commands:
                    if exchange(child, identity, dict(type=command))['type'] != expected:
                        raise ValueError(f'Wrong native {command} reply')
                if attempt == 0:
                    def commit(snapshot, receipt):
                        records = fixture / 'machines/records'
                        records.mkdir(parents=True)
                        (records / f'{machine}.json').write_text(json.dumps(dict(
                            id=machine, name='Synthetic snapshot probe', guest='windows', profile='windows',
                            resources=dict(cpus=2, memoryGib=2, diskGib=64), agentAccess=True,
                            agentGeneration=17, runtime='hypervisor', installer=None)))
                        (fixture / 'snapshots.json').write_text(json.dumps(dict(
                            machine=machine, configuration=configuration, snapshot=snapshot, receipt=receipt)))
                        subprocess.run([str(root / 'target/debug/examples/snapshotcommit'),
                                        str(fixture), 'commit'], check=True, timeout=30)
                    handoff = workersnapshot.check(child, exchange, fixture, commit)
                    handoff.update(machine=machine, configuration=configuration)
                if child.wait(timeout=10) != 0:
                    raise ValueError('Native worker did not exit successfully')
                child.stdin.close()
                child.stdout.close()
        (fixture / 'snapshots.json').write_text(json.dumps(handoff))
        subprocess.run([str(root / 'target/debug/examples/snapshotcommit'), str(fixture), 'deny'],
                       check=True, timeout=30)
        if (fingerprints[0] != fingerprints[1] or fingerprints[0] == fingerprints[2]
                or fingerprints[0] == fingerprints[3]):
            raise ValueError('Native hardware fingerprint is unstable or omits CPU/firmware changes')
        print('Signed worker identity/lifecycle and stable fresh-process hardware fingerprints pass; '
              'CPU and firmware changes differ')
    except BaseException:
        print(f'Failed worker fixture preserved: {fixture}', flush=True)
        raise
    else:
        shutil.rmtree(fixture)
    finally:
        if child is not None and child.poll() is None:
            child.kill()
            child.wait(timeout=10)


if __name__ == '__main__':
    main()
