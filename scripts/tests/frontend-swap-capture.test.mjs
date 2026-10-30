import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

test('capture reads tab ids from the CLI result array across workspaces', () => {
  const source = readFileSync(new URL('../../evals/frontend-swap/capture.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  assert.match(source, /dc tab list --json/);
  assert.doesNotMatch(source, /--all-workspaces/);
  const consumer = source.match(/<<'PY'\n([\s\S]*?)\nPY/)[1];
  const directory = mkdtempSync(path.join(os.tmpdir(), 'frontend-capture-'));
  try {
    const input = path.join(directory, 'tabs.json');
    writeFileSync(input, JSON.stringify({ result: [
      { id: 'first', scope_workspace_id: 'one' }, { id: 'second', scope_workspace_id: 'two' },
    ], meta: { scope: 'all_workspaces' } }));
    const python = process.platform === 'win32' ? 'python' : 'python3';
    const result = spawnSync(python, ['-c', consumer, input], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.replaceAll('\r\n', '\n'), 'first\nsecond\n');
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
