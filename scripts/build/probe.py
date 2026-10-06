#!/usr/bin/env python3
import json
import pathlib
import socket
import subprocess
import sys
import tempfile
import time


def probe(runtime):
    runtime = pathlib.Path(runtime).resolve()
    with tempfile.TemporaryDirectory(prefix='hoppervm-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'tpm').mkdir()
        tpm = subprocess.Popen([
            str(runtime / 'bin/swtpm'), 'socket', '--tpm2', '--tpmstate',
            f'dir={root}/tpm', '--ctrl', f'type=unixio,path={root}/tpm.sock',
            '--flags', 'not-need-init',
        ], stderr=subprocess.PIPE)
        qemu = None
        try:
            for _ in range(100):
                if (root / 'tpm.sock').exists():
                    break
                time.sleep(0.05)
            qemu = subprocess.Popen([
                str(runtime / 'bin/qemu-system-aarch64'), '-machine', 'virt,accel=hvf',
                '-cpu', 'host', '-m', '1024', '-smp', '1', '-display', 'none',
                '-bios', str(runtime / 'share/qemu/edk2-aarch64-code.fd'),
                '-chardev', f'socket,id=chrtpm,path={root}/tpm.sock',
                '-tpmdev', 'emulator,id=tpm0,chardev=chrtpm',
                # Lima disables the small PPI region for QEMU 11 with 16 KiB host pages.
                '-device', 'tpm-tis-device,tpmdev=tpm0,ppi=off',
                '-qmp', f'unix:{root}/qmp.sock,server=on,wait=off',
            ], stderr=subprocess.PIPE)
            for _ in range(100):
                if (root / 'qmp.sock').exists():
                    break
                if qemu.poll() is not None:
                    raise RuntimeError(qemu.stderr.read().decode())
                time.sleep(0.05)
            with socket.socket(socket.AF_UNIX) as connection:
                connection.settimeout(5)
                connection.connect(str(root / 'qmp.sock'))
                stream = connection.makefile('rwb', buffering=0)
                if 'QMP' not in json.loads(stream.readline()):
                    raise RuntimeError('Missing QMP greeting')

                def command(name):
                    stream.write((json.dumps({'execute': name}) + '\n').encode())
                    while True:
                        line = stream.readline()
                        if not line:
                            raise RuntimeError(qemu.stderr.read().decode())
                        value = json.loads(line)
                        if 'return' in value:
                            return value['return']
                        if 'error' in value:
                            raise RuntimeError(value['error'])

                command('qmp_capabilities')
                if not command('query-status')['running']:
                    raise RuntimeError('QEMU did not start')
                time.sleep(1)
                if qemu.poll() is not None:
                    raise RuntimeError(qemu.stderr.read().decode())
                command('quit')
                qemu.wait(timeout=5)
            print('[qemu] firmware, hardware virtualization, and TPM2 verified')
        finally:
            for process in [qemu, tpm]:
                if process and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)


if __name__ == '__main__':
    probe(sys.argv[1])
