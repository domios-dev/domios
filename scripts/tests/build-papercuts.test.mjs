import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import test from 'node:test';
import { compilerInputs } from '../build-compiler-inputs.mjs';

const python = process.platform === 'win32' ? 'python' : 'python3';

test('compiler diagnostics record effective directories and only allowed environment inputs', () => {
  const calls = [];
  const snapshot = compilerInputs(['--manifest-path', 'fixture/Cargo.toml', '-p', 'fixture'], {
    cwd: '/stable', env: { CT_REAL_WORKTREE: '/physical', CARGO_BUILD_BUILD_DIR: '/cache', RUSTC_WRAPPER: '/wrapper', CODETERM_API_TOKEN: 'must-not-record' },
    runner(command, args, options) {
      calls.push({ command, args, cwd: options.cwd });
      return { status: 0, stdout: JSON.stringify({ workspace_root: '/stable/fixture', build_directory: '/cache', target_directory: '/stable/fixture/target' }) };
    },
  });
  assert.equal(snapshot.worktree, '/physical');
  assert.equal(snapshot.build_directory, '/cache');
  assert.equal(snapshot.environment.RUSTC_WRAPPER, '/wrapper');
  assert.equal(snapshot.environment.CARGO_INCREMENTAL, null);
  assert.equal(JSON.stringify(snapshot).includes('must-not-record'), false);
  assert.deepEqual(calls, [{ command: 'cargo', args: ['metadata', '--offline', '--no-deps', '--format-version=1', '--manifest-path', 'fixture/Cargo.toml'], cwd: '/stable' }]);
  assert.throws(() => compilerInputs([], { runner: () => ({ status: 1, stderr: 'fixture error' }) }), /fixture error/);
});

test('lane checks refuse concurrent shared-intermediate ownership and release on process exit', { timeout: 20000 }, async () => {
  const env = { ...process.env, CODETERM_BUILD_GATEWAY_MODE: 'fail-fast' };
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'lane-lock-'));
  const shared = path.join(directory, 'lanes');
  fs.mkdirSync(shared);
  fs.writeFileSync(path.join(shared, 'CACHEDIR.TAG'), 'fixture');
  fs.writeFileSync(path.join(directory, 'paths.json'), '[]');
  const helper = path.resolve(import.meta.dirname, '../lane-check-run.py');
  const holder = spawn(python, ['-u', helper, shared, process.execPath, '-e',
    'process.stdout.write("READY\\n"); require("node:fs").readSync(0, Buffer.alloc(1), 0, 1, null);'], { env, stdio: ['pipe', 'pipe', 'pipe'] });
  let error = '';
  holder.stderr.on('data', chunk => error += chunk);
  const exit = once(holder, 'close');
  const lines = readline.createInterface({ input: holder.stdout });
  try {
    const ready = await Promise.race([once(lines, 'line').then(([line]) => line), exit.then(() => { throw new Error(error); })]);
    assert.equal(ready, 'READY');
    const refused = spawnSync(python, [helper, shared, process.execPath, '-e', 'process.exit(99)'], { env, encoding: 'utf8' });
    assert.equal(refused.status, 75, refused.stderr);
    assert.match(refused.stderr, /lane-check busy/);
    const trim = spawnSync(python, [path.resolve(import.meta.dirname, '../cargo-build-gc.py'), '--trim', directory, shared, path.join(directory, 'paths.json')], { env, encoding: 'utf8' });
    assert.equal(trim.status, 0, trim.stderr);
    assert.equal(JSON.parse(trim.stdout).status, 'protected');
    const independent = spawnSync(python, [helper, path.join(directory, 'other'), process.execPath, '-e', 'process.exit(37)'], { env, encoding: 'utf8' });
    assert.equal(independent.status, 37, independent.stderr);
    holder.stdin.end('x');
    assert.equal((await exit)[0], 0, error);
    const released = spawnSync(python, [helper, shared, process.execPath, '-e', 'process.exit(41)'], { env, encoding: 'utf8' });
    assert.equal(released.status, 41, released.stderr);
  } finally {
    holder.stdin.end('x');
    await exit;
    lines.close();
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
