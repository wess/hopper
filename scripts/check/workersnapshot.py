"""private native snapshot staging; final publication belongs to the authorized host."""

import hashlib
import time
import uuid


def check(child, exchange, fixture, commit):
    sequence = 20

    def call(kind, snapshot=None, policy=17):
        nonlocal sequence
        sequence += 1
        command = dict(type=kind)
        if snapshot is not None:
            command.update(snapshotId=snapshot, policyGeneration=policy)
        return exchange(child, sequence, command)

    def start():
        snapshot = str(uuid.uuid4())
        if call('snapshotStart', snapshot)['type'] != 'accepted':
            raise ValueError('Native worker did not accept snapshot staging')
        return snapshot

    def ready(snapshot):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            reply = call('snapshotPoll', snapshot)
            if reply['type'] != 'snapshot' or reply['snapshotId'] != snapshot:
                raise ValueError(f'Wrong snapshot identity: {reply}')
            status = reply['status']
            if status['state'] == 'ready':
                receipt = status['receipt']
                stage = fixture / 'snapshots' / ('.stage' + bytes(receipt['nonce']).hex())
                if (stage.stat().st_ino != receipt['inode']
                        or stage.stat().st_dev != receipt['device']
                        or hashlib.sha256((stage / 'manifest.hsm').read_bytes()).digest()
                        != bytes(receipt['manifest'])):
                    raise ValueError('Snapshot receipt does not bind the actual stage')
                if (fixture / 'snapshots' / uuid.UUID(snapshot).hex).exists():
                    raise ValueError('Native worker published a snapshot')
                return receipt
            if status['state'] not in ('capturing', 'staging'):
                raise ValueError(f'Snapshot staging failed: {status}')
            time.sleep(0.02)
        raise TimeoutError('Native snapshot staging did not complete')

    decoy = fixture / 'snapshotdecoy'
    decoy.mkdir(mode=0o700)
    namespace = fixture / 'snapshots'
    namespace.symlink_to(decoy, target_is_directory=True)
    if call('snapshotStart', str(uuid.uuid4()))['type'] != 'rejected' or list(decoy.iterdir()):
        raise ValueError('Snapshot namespace symlink redirected staging')
    namespace.unlink()
    snapshot = start()
    if call('snapshotPoll', snapshot, policy=18)['type'] != 'rejected':
        raise ValueError('Another policy generation renewed a snapshot')
    receipt = ready(snapshot)
    if call('status')['paused']:
        raise ValueError('Running snapshot did not resume source hardware')
    if call('snapshotStart', str(uuid.uuid4()))['type'] != 'rejected':
        raise ValueError('Native worker admitted concurrent snapshot jobs')
    if call('snapshotCancel', snapshot, policy=18)['type'] != 'rejected':
        raise ValueError('Another policy generation cancelled a snapshot')
    if ready(snapshot) != receipt:
        raise ValueError('Ready snapshot receipt changed')
    commit(snapshot, receipt)
    if call('snapshotCancel', snapshot)['type'] != 'accepted':
        raise ValueError('Snapshot cancellation failed')
    if call('snapshotCancel', snapshot)['type'] != 'accepted':
        raise ValueError('Repeated snapshot cleanup was not idempotent')
    if call('snapshotPoll', snapshot)['type'] != 'rejected':
        raise ValueError('Cancelled snapshot could still be polled')

    if call('pause')['type'] != 'paused':
        raise ValueError('Could not pause before snapshot')
    paused = start()
    paused_receipt = ready(paused)
    if not call('status')['paused']:
        raise ValueError('Snapshot resumed a manually paused VM')
    call('snapshotCancel', paused)
    call('resume')

    expired = start()
    time.sleep(5.3)
    status = call('snapshotPoll', expired)
    if status.get('status', {}).get('state') != 'failed':
        raise ValueError('Unrenewed staging lease did not fail permanently')
    call('snapshotCancel', expired)
    start()
    if call('stop')['type'] != 'stopped':
        raise ValueError('Stop did not cancel and join snapshot work')
    print('Signed worker snapshot staging, receipt binding, source pause preservation, '
          'job exclusion, generation checks, lease expiry and stop cleanup passed', flush=True)
    return dict(snapshot=paused, receipt=paused_receipt)
