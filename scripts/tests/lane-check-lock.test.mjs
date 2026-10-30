import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
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
run.__globals__['run_cargo'] = lambda command: calls.append(command) or 17
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

test('lane check streams child output, forwards stdin and preserves the command exit code', { timeout: 15000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'lane-check-stream-'));
  const python = process.platform === 'win32' ? 'python' : 'python3';
  const fixture = String.raw`
import json, sys
print('READY', flush=True)
assert sys.stdin.read(1) == 'x'
print(json.dumps({'reason': 'compiler-message', 'message': {'level': 'warning', 'rendered': 'DIAGNOSTIC\n'}}), flush=True)
print('CHILD-STDERR', file=sys.stderr, flush=True)
sys.exit(17)
`;
  const env = { ...process.env };
  delete env.CODETERM_BUILD_GATEWAY_MODE;
  const child = spawn(python, ['-u', path.resolve('scripts/lane-check-run.py'), directory, python, '-u', '-c', fixture], { env, timeout: 10000 });
  const closed = once(child, 'close');
  const lines = readline.createInterface({ input: child.stdout });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', chunk => { stdout += chunk; });
  child.stderr.on('data', chunk => { stderr += chunk; });
  try {
    const first = await Promise.race([
      once(lines, 'line').then(([line]) => line),
      closed.then(([code]) => { throw new Error(`child exited before READY: ${code}: ${stderr}`); }),
    ]);
    assert.equal(first, 'READY');
    assert.equal(child.exitCode, null);
    child.stdin.end('x');
    const [code, signal] = await closed;
    assert.equal(signal, null);
    assert.equal(code, 17, stderr);
    assert.equal(stdout.replaceAll('\r\n', '\n'), 'READY\nDIAGNOSTIC\n');
    assert.match(stderr, /CHILD-STDERR/);
  } finally {
    child.stdin.end();
    if (child.exitCode === null && child.signalCode === null) child.kill();
    await closed;
    lines.close();
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
