import assert from 'node:assert/strict';
import test from 'node:test';
import { fastCliTargets } from '../ci-cli-targets.mjs';

test('the fast CLI suite retains every target except the separately owned real server pair', () => {
  const targets = [
    { name: 'codeterm', kind: ['bin'] }, { name: 'opaque_contract', kind: ['test'] },
    { name: 'isolated_server', kind: ['test'] }, { name: 'build-script-build', kind: ['custom-build'] },
  ];
  assert.deepEqual(fastCliTargets({ packages: [{ name: 'codeterm-cli', targets }] }), ['--bin', 'codeterm', '--test', 'opaque_contract']);
  assert.throws(() => fastCliTargets({ packages: [] }), /Missing/);
  assert.throws(() => fastCliTargets({ packages: [{ name: 'codeterm-cli', targets: [] }] }), /No fast/);
});
