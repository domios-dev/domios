import assert from 'node:assert/strict';
import test from 'node:test';
import { validateDevInstance } from '../dev-instance-policy.mjs';

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
  assert.throws(() => validateDevInstance({ worktree: true, instance: 'lane' }), /own CODETERM_API_PORT/);
  assert.doesNotThrow(() => validateDevInstance({ worktree: true, instance: 'lane', port: '7799' }));
});

test('canonical default remains available and invalid ports refuse', () => {
  assert.doesNotThrow(() => validateDevInstance({ worktree: false }));
  for (const port of ['0', '65536', 'garbage', '-1', '1.5']) {
    assert.throws(() => validateDevInstance({ worktree: false, port }), /valid nonzero/);
  }
});
