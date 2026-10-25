import assert from 'node:assert/strict';
import test from 'node:test';
import { dispatchQualifiedRelease } from '../dispatch-qualified-release.mjs';

const sha = 'a'.repeat(40);
function fixture({ owner = 'rollacode', heads = [sha, sha], qualify = () => {} } = {}) {
  const calls = [];
  return { calls, options: { qualify, run: args => {
    calls.push(args);
    if (args[0] === 'workflow') return '';
    return args[1] === 'user' ? owner : heads.shift();
  } } };
}

test('owner dispatch pins the qualified source and rehearses without publication', () => {
  const f = fixture();
  assert.equal(dispatchQualifiedRelease(sha, 'release/9.9.9', f.options), sha);
  const args = f.calls.at(-1);
  for (const value of ['release.yml', 'release/9.9.9', 'tag=' + sha, 'dry_run=true', 'mirror=false']) assert.ok(args.includes(value));
});

test('unqualified, foreign, superseded and moving sources cannot dispatch', () => {
  for (const options of [{ owner: 'contributor' }, { heads: ['b'.repeat(40)] },
    { heads: [sha, 'b'.repeat(40)] }, { qualify: () => { throw new Error('not green'); } }]) {
    const f = fixture(options);
    assert.throws(() => dispatchQualifiedRelease(sha, 'release/9.9.9', f.options));
    assert.ok(f.calls.every(args => args[0] !== 'workflow'));
  }
});
