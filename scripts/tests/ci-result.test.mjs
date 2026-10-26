import assert from 'node:assert/strict';
import test from 'node:test';
import { ciFailures } from '../ci-result.mjs';

const graph = event => ({
  policy: { result: 'success' }, pr: { result: event === 'pull_request' ? 'success' : 'skipped' },
  fast: { result: event === 'pull_request' ? 'skipped' : 'success', outputs: { heavy: 'true' } },
  rust: { result: event === 'pull_request' ? 'skipped' : 'success' },
  'rust-warnings': { result: event === 'pull_request' ? 'skipped' : 'success' },
  qualification: { result: event === 'pull_request' ? 'skipped' : 'success' },
});


test('legitimate scope skips pass; failed optional jobs and missing graph results never pass', () => {
  const needs = graph('push');
  needs.fast.outputs.heavy = 'false';
  needs.rust.result = 'skipped';
  needs.qualification.result = 'skipped';
  needs['rust-warnings'].result = 'skipped';
  assert.deepEqual(ciFailures('push', needs), []);
  needs['rust-warnings'].result = 'failure';
  assert.deepEqual(ciFailures('push', needs), ['rust-warnings']);
  assert.ok(ciFailures('push', {}).length);
});
