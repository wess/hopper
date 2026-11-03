"""verify configured native cold start and real PSCI reset supervision in an isolated fixture."""

import os
import pathlib
import platform
import shutil
import subprocess
import tempfile
import uuid


def main(prepared=False):
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        raise SystemExit('Configured generation checks require Apple silicon macOS')
    root = pathlib.Path(__file__).resolve().parents[2]
    subprocess.run(['cargo', 'build', '-p', 'machine', '--bin', 'hoppervm', '-p', 'engine',
                    '--example', 'generationstart', '--locked'], cwd=root,
                   env=dict(os.environ, CARGO_INCREMENTAL='0'), check=True)
    fixture = pathlib.Path(tempfile.mkdtemp(prefix='hoppergeneration.'))
    try:
        worker = fixture / 'hoppervm'
        shutil.copy2(root / 'target/debug/hoppervm', worker)
        subprocess.run(['codesign', '--force', '--sign', '-', '--entitlements',
                        str(root / 'assets/machine.entitlements'), str(worker)], check=True)
        subprocess.run(['codesign', '--verify', '--strict', str(worker)], check=True)
        subprocess.run([str(root / 'target/debug/examples/generationstart'),
                        str(fixture), str(uuid.uuid4()), str(worker)],
                       check=True, timeout=60,
                       env=dict(os.environ, HOPPER_GENERATION_PREPARED="1" if prepared else "0"))
    except BaseException:
        print(f'Failed configured generation fixture preserved: {fixture}', flush=True)
        raise
    else:
        shutil.rmtree(fixture)


if __name__ == '__main__':
    main()
    main(prepared=True)
