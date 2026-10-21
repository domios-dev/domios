import assert from 'node:assert/strict';
import test from 'node:test';
import { coreUnits } from '../feature-cycle-units.mjs';

test('cycle counts completed core units once across stages, filenames and fresh artifacts', () => {
  const artifact = (hash, features, fresh = false) => JSON.stringify({ reason: 'compiler-artifact', target: { name: 'codeterm_service_core' }, features, fresh, profile: { test: false }, filenames: [`C:\\cache\\deps\\libcodeterm_service_core-${hash}.rmeta`, `/cache/deps/libcodeterm_service_core-${hash}.rlib`] });
  const a = 'a'.repeat(16), b = 'b'.repeat(16);
  const units = coreUnits({ dev: artifact(a, ['opaque']), clippy: artifact(a, ['opaque'], true), test: artifact(b, []) + '\nnot json\n' + JSON.stringify({ reason: 'compiler-artifact', target: { name: 'other' }, filenames: [] }) });
  assert.equal(units.length, 2);
  assert.deepEqual(units[0].phases, ['dev', 'clippy']);
  assert.deepEqual(units[1].features, []);
});
