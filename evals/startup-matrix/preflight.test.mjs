import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, resolve } from 'node:path';
import { healthPreflight, assertDevHealth, classifyPreflight } from './preflight.mjs';
import { render } from './model.mjs';
import { validate } from './contract.mjs';

const health = { status: 'ok', build: 'fixture', instance: { name: '', api_port: '7686', vite_port: '1420', cdp_port: '9223' } };
test('slow health is a failure row while its response remains available to the matrix', async () => {
  let now = 100, calls = 0;
  const result = await healthPreflight({ now: () => now, read: async signal => {
    calls++; assert.equal(signal.aborted, false); now += 15000; return health;
  } });
  assert.equal(calls, 1);
  assert.equal(result.id, 'api_unresponsive'); assert.equal(result.verdict, 'fail');
  assert.equal(result.ms, 15000); assert.equal(result.responded, true); assert.deepEqual(result.health, health);
  assert.doesNotThrow(() => assertDevHealth(result.health));
});
test('one request deadline becomes evidence without a retry', async () => {
  let now = 0, calls = 0;
  const result = await healthPreflight({ now: () => now, read: async () => {
    calls++; now = 30001; throw new Error('fixture request deadline');
  } });
  assert.equal(calls, 1); assert.equal(result.responded, false); assert.equal(result.ms, 30001);
  assert.equal(result.deadline_ms, 30000); assert.equal(result.error, 'fixture request deadline');
});
test('fast health and exact threshold pass without concealing slower responses', () => {
  assert.equal(classifyPreflight({ phase: 'entry', at: 0, ms: 5000, health }).verdict, 'pass');
  assert.equal(classifyPreflight({ phase: 'entry', at: 0, ms: 5001, health }).verdict, 'fail');
});
test('late health must still prove the default DEV target before any UI action', () => {
  for (const instance of [{ ...health.instance, api_port: '7685' }, { ...health.instance, name: 'other' },
    { ...health.instance, cdp_port: '9222' }, { ...health.instance, vite_port: '1421' }])
    assert.throws(() => assertDevHealth({ ...health, instance }), /Refused/);
});
test('unresponsive infrastructure has a valid failing contract and a visible failure row', () => {
  const dir = mkdtempSync(join(tmpdir(), 'startup-preflight-'));
  try {
    const preflight = classifyPreflight({ phase: 'entry', at: 0, ms: 30001, error: 'fixture timeout' });
    const result = { schema: 1, phase: 'preflight', build: 'fixture', finished_at: 1, preflights: [preflight],
      tabs: [], coverage: [], failures: ['fixture timeout'] };
    writeFileSync(join(dir, 'results.json'), JSON.stringify(result));
    writeFileSync(join(dir, 'results.md'), render(result));
    writeFileSync(join(dir, 'preflight-entry.json'), JSON.stringify(preflight));
    writeFileSync(join(dir, 'dev.log'), ''); writeFileSync(join(dir, 'lock-holds.md'), '');
    assert.deepEqual(validate(dir), { errors: [], failed: true });
    assert.match(render(result), /api_unresponsive.*FAILURE 30001\/5000 ms/);
  } finally {
    assert.equal(dirname(resolve(dir)), resolve(tmpdir()));
    rmSync(dir, { recursive: true, force: true });
  }
});
