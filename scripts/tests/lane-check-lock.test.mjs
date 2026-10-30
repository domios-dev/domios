import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';

test('lane check refuses a busy lock by default and waits in queue mode before running once', () => {
  const python = process.platform === 'win32' ? 'python' : 'python3';
  const fixture = String.raw`
import contextlib, errno, os, runpy, sys, tempfile, threading
runner = runpy.run_path(sys.argv[1])
run = runner['run']
entered = threading.Event()
released = threading.Event()
calls = []
@contextlib.contextmanager
def occupied(file, blocking=False):
    if not blocking:
        raise BlockingIOError(errno.EAGAIN, 'occupied')
    entered.set()
    assert released.wait(5), 'lock release event did not arrive'
    yield
run.__globals__['runpy'].run_path = lambda _: {'cargo_lock': occupied}
run.__globals__['subprocess'].call = lambda command: calls.append(command) or 17
with tempfile.TemporaryDirectory() as directory:
    os.environ.pop('CODETERM_BUILD_GATEWAY_MODE', None)
    assert run(directory, ['opaque-command']) == 75
    assert calls == []
    os.environ['CODETERM_BUILD_GATEWAY_MODE'] = 'queue'
    result = []
    worker = threading.Thread(target=lambda: result.append(run(directory, ['opaque-command'])))
    worker.start()
    assert entered.wait(5), 'queue mode did not enter a blocking lock wait'
    assert calls == [], 'command ran before lock ownership'
    released.set()
    worker.join(5)
    assert not worker.is_alive(), 'queued check did not finish after lock release'
    assert result == [17]
    assert calls == [['opaque-command']]
`;
  const result = spawnSync(python, ['-c', fixture, path.resolve('scripts/lane-check-run.py')], { encoding: 'utf8', timeout: 15000 });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stderr, /lane-check busy/);
  assert.match(result.stdout, /lane-check queued/);
});
