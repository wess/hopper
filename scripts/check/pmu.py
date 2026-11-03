"""run the native PMU fault/timer checks with a signed test executable."""

import json
import os
import pathlib
import platform
import subprocess


def main():
  if platform.system() != 'Darwin' or platform.machine() != 'arm64':
    raise SystemExit('Native PMU checks require Apple silicon macOS')
  root = pathlib.Path(__file__).resolve().parents[2]
  environment = dict(os.environ, CARGO_INCREMENTAL='0')
  result = subprocess.run(
    ['cargo', 'test', '-p', 'machine', '--lib', '--locked', '--no-run', '--message-format=json'],
    cwd=root, env=environment, check=True, stdout=subprocess.PIPE, text=True,
  )
  artifacts = []
  for line in result.stdout.splitlines():
    item = json.loads(line)
    if (item.get('reason') == 'compiler-artifact'
        and item.get('target', {}).get('name') == 'machine'
        and item.get('target', {}).get('kind') == ['lib']
        and item.get('profile', {}).get('test')
        and item.get('executable')):
      artifacts.append(item['executable'])
  if len(artifacts) != 1:
    raise ValueError('Expected one machine library test executable')
  binary = artifacts[0]
  subprocess.run(
    ['codesign', '--force', '--sign', '-', '--entitlements',
     str(root / 'assets/machine.entitlements'), binary],
    cwd=root, check=True,
  )
  subprocess.run(['codesign', '--verify', '--strict', binary], cwd=root, check=True)
  for test in ['hypervisor::pmu::tests::fault_and_timers',
               'hypervisor::pmu::cancel::interrupted_access',
               'hypervisor::pmu::cancel::canceled_checkpoint_restarts_fixed_access',
               'hypervisor::pmu::cancel::checkpoint_fallthrough_stays_in_access']:
    subprocess.run(
      [binary, '--exact', test, '--ignored', '--nocapture', '--test-threads=1'],
      cwd=root, check=True, timeout=30,
    )


if __name__ == '__main__':
  main()
