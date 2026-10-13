import assert from 'node:assert/strict';
import test from 'node:test';
import { checkSlowTests, parseTimings, validateBaseline } from './check-slow-unit-tests.mjs';

const entry = (name, seconds) => ({ binary: 'codeterm_lib', test: name, measuredSeconds: seconds });
const baseline = (...entries) => ({ version: 1, entries });

test('parses nextest timings and keeps the slowest retry', () => {
  assert.deepEqual(parseTimings('\x1b[32m PASS [ 1.200s] codeterm_lib a\x1b[0m\n FAIL [ 0.200s] codeterm_lib a\n PASS [ 0.010s] codeterm_lib b\n Summary [ 1.210s] 2 tests run: 2 passed\n'), [entry('a', 1.2), entry('b', 0.01)]);
});

test('rejects incomplete and empty reports', () => {
  assert.throws(() => parseTimings('PASS [ 1.200s] codeterm_lib a'), /summary/);
  assert.throws(() => parseTimings('Summary [ 0.000s] 0 tests run'), /no per-test/);
});

test('new slow unit fails and one second is permitted', () => {
  assert.deepEqual(checkSlowTests([entry('a', 1)], baseline()), []);
  assert.match(checkSlowTests([entry('a', 1.001)], baseline())[0], /new slow unit/);
});

test('existing slow units remain visible and can become fast', () => {
  assert.deepEqual(checkSlowTests([entry('a', 2)], baseline(entry('a', 1.5))), []);
  assert.deepEqual(checkSlowTests([entry('a', 0.2)], baseline(), baseline(entry('a', 1.5))), []);
});

test('baseline cannot gain tests or increase recorded durations', () => {
  assert.match(checkSlowTests([], baseline(entry('a', 2)), baseline())[0], /added baseline/);
  assert.match(checkSlowTests([], baseline(entry('a', 2)), baseline(entry('a', 1.5)))[0], /increased baseline/);
  assert.deepEqual(checkSlowTests([], baseline(entry('a', 1.1)), baseline(entry('a', 1.5))), []);
});

test('baseline identities are exact and unique', () => {
  assert.throws(() => validateBaseline(baseline(entry('a', 1.5), entry('a', 1.5))), /duplicate/);
  assert.throws(() => validateBaseline(baseline(entry('a', 0.5))), /exact/);
  assert.throws(() => validateBaseline({ version: 1, entries: [], wildcard: true }), /only/);
});
