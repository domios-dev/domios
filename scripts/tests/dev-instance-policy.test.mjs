import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync, mkdtempSync, mkdirSync, copyFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { validateDevInstance, devLaunchSelection } from '../dev-instance-policy.mjs';

test('production port is refused for every development instance', () => {
  for (const worktree of [true, false]) {
    for (const instance of ['', 'lane']) {
      for (const port of ['7685', '07685']) {
        assert.throws(() => validateDevInstance({ worktree, instance, port }), /production port/);
      }
    }
  }
});

test('worktree must select both a name and private port', () => {
  assert.throws(() => validateDevInstance({ worktree: true }), /non-canonical/);
  assert.throws(() => validateDevInstance({ worktree: true, port: '7799' }), /non-canonical/);
  assert.throws(() => validateDevInstance({ worktree: true, instance: 'lane' }), /own CODETERM_DEV_API_PORT/);
  assert.doesNotThrow(() => validateDevInstance({ worktree: true, instance: 'lane', port: '7799' }));
});

test('launch selection ignores inherited parent connection selectors', () => {
  const inherited = { CODETERM_API_PORT: '7685', CODETERM_INSTANCE: 'parent', CODETERM_API_URL: 'opaque', CODETERM_API_TOKEN: 'opaque' };
  assert.deepEqual(devLaunchSelection(inherited, false), { instance: '', port: '7686' });
  assert.throws(() => devLaunchSelection(inherited, true), /non-canonical/);
  assert.deepEqual(devLaunchSelection({ ...inherited, CODETERM_DEV_INSTANCE: 'lane', CODETERM_DEV_API_PORT: '7799' }, true),
    { instance: 'lane', port: '7799' });
  assert.throws(() => devLaunchSelection({ ...inherited, CODETERM_DEV_API_PORT: '7685' }, false), /production port/);
});

test('build launcher scrubs parent connection and exports default or explicit DEV selection', () => {
  const root = path.resolve(import.meta.dirname, '../..');
  const source = readFileSync(path.join(root, 'build.sh'), 'utf8').replaceAll('\r\n', '\n');
  const isolation = source.slice(source.indexOf('# --- Per-worktree dev instance isolation ---'),
    source.indexOf('\nif [ "$CMD" = "dev" ] || [ "$CMD" = "dev-prebuilt" ]; then', source.indexOf('# --- Per-worktree dev instance isolation ---')));
  const scrub = source.match(/case "\$CMD" in\n  dev\|dev-prebuilt\|dev-server\|dev-front\|kill\|e2e\|e2e:llm\)\n    unset[^]*?\nesac/)[0];
  const directory = mkdtempSync(path.join(tmpdir(), 'dev-selection-'));
  const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
  try {
    mkdirSync(path.join(directory, 'scripts'));
    copyFileSync(path.join(root, 'scripts/dev-instance-policy.mjs'), path.join(directory, 'scripts/dev-instance-policy.mjs'));
    const init = spawnSync('git', ['init', '--quiet'], { cwd: directory, encoding: 'utf8' });
    assert.equal(init.status, 0, init.stderr);
    for (const explicit of [false, true]) {
      const env = { ...process.env, CODETERM_API_PORT: '7685', CODETERM_INSTANCE: 'parent',
        CODETERM_API_URL: 'http://parent.invalid:7685', CODETERM_API_TOKEN: 'parent-token',
        CODETERM_SOCKET_PATH: 'parent-socket', CODETERM_DATA_DIR: 'parent-data',
        CODETERM_VITE_PORT: '9991', CODETERM_CDP_PORT: '9992' };
      for (const key of ['CODETERM_DEV_INSTANCE', 'CODETERM_DEV_API_PORT', 'CODETERM_DEV_VITE_PORT', 'CODETERM_DEV_CDP_PORT']) delete env[key];
      if (explicit) Object.assign(env, { CODETERM_DEV_INSTANCE: 'lane', CODETERM_DEV_API_PORT: '7799' });
      const result = spawnSync(bash, ['-c', `set -eu\nCMD=dev\n${isolation}\n${scrub}\nnode -e 'console.log(JSON.stringify(process.env))'`],
        { cwd: directory, env, encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      const selected = JSON.parse(result.stdout);
      assert.equal(selected.CODETERM_INSTANCE, explicit ? 'lane' : '');
      assert.equal(selected.CODETERM_API_PORT, explicit ? '7799' : '7686');
      assert.equal(selected.CODETERM_DEV_INSTANCE, selected.CODETERM_INSTANCE);
      assert.equal(selected.CODETERM_DEV_API_PORT, selected.CODETERM_API_PORT);
      for (const key of ['CODETERM_API_URL', 'CODETERM_API_TOKEN', 'CODETERM_SOCKET_PATH', 'CODETERM_DATA_DIR']) assert.equal(selected[key], undefined);
      assert.notEqual(selected.CODETERM_VITE_PORT, '9991');
      assert.notEqual(selected.CODETERM_CDP_PORT, '9992');
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('canonical default remains available and invalid ports refuse', () => {
  assert.doesNotThrow(() => validateDevInstance({ worktree: false }));
  for (const port of ['0', '65536', 'garbage', '-1', '1.5']) {
    assert.throws(() => validateDevInstance({ worktree: false, port }), /valid nonzero/);
  }
});
