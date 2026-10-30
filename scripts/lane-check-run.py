import contextlib
import os
from pathlib import Path
import runpy
import subprocess
import sys


def run(directory, command):
    root = Path(directory)
    root.mkdir(parents=True, exist_ok=True)
    lock = root / '.domios-lane-check.lock'
    with lock.open('ab'):
        pass
    cargo_lock = runpy.run_path(str(Path(__file__).with_name('cargo-build-gc.py')))['cargo_lock']
    with contextlib.ExitStack() as stack:
        try:
            queued = os.environ.get('CODETERM_BUILD_GATEWAY_MODE') == 'queue'
            if queued:
                print(f'lane-check queued: waiting for shared intermediates: {root}', flush=True)
            stack.enter_context(cargo_lock(lock, blocking=queued))
        except OSError as error:
            if error.errno in (11, 13) or getattr(error, 'winerror', None) in (32, 33):
                print(f'lane-check busy: shared intermediates are owned by another check: {root}', file=sys.stderr)
                return 75
            raise
        return subprocess.call(command)


if __name__ == '__main__':
    sys.exit(run(sys.argv[1], sys.argv[2:]))
