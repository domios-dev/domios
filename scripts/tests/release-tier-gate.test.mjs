import assert from 'node:assert/strict';
import test from 'node:test';
import { tierState, waitForTier } from '../release-tier-gate.mjs';

const tier = { prefix: 'Opaque tier / ', anchor: 'Opaque tier / first' };
const job = (name, status) => ({ name: `Opaque tier / ${name}`, status });

test('a tier opens only after its anchor finished and no job of it waits for a runner', () => {
  assert.equal(tierState([], tier).open, false);
  assert.equal(tierState([job('first', 'in_progress')], tier).open, false);
  assert.equal(tierState([job('first', 'completed'), job('a', 'queued')], tier).open, false);
  assert.equal(tierState([job('first', 'completed'), job('a', 'waiting')], tier).open, false);
  assert.equal(tierState([job('first', 'completed'), job('a', 'in_progress'), job('b', 'completed')], tier).open, true);
  assert.equal(tierState([job('first', 'completed'), { name: 'Other tier / x', status: 'queued' }], tier).open, true);
});

test('the gate needs two consecutive open readings and reports why it waited', async () => {
  const readings = [
    [job('first', 'in_progress')],
    [job('first', 'completed')],
    [job('first', 'completed'), job('a', 'queued')],
    [job('first', 'completed'), job('a', 'in_progress')],
    [job('first', 'completed'), job('a', 'in_progress')],
  ];
  let clock = 0;
  const waits = [];
  const result = await waitForTier({ ...tier, fetchJobs: async () => readings.shift(), sleep: async ms => { clock += ms; },
    now: () => clock, intervalMs: 10, deadlineMs: 1000, log: line => waits.push(line) });
  assert.equal(result.opened, true);
  assert.equal(readings.length, 0);
  assert.match(waits.join('\n'), /unclaimed: Opaque tier \/ a/);
});

test('a tier that is never claimed opens the next one at the deadline instead of deadlocking it', async () => {
  let clock = 0;
  const result = await waitForTier({ ...tier, fetchJobs: async () => [job('first', 'completed'), job('a', 'queued')],
    sleep: async ms => { clock += ms; }, now: () => clock, intervalMs: 10, deadlineMs: 50, log: () => {} });
  assert.deepEqual(result, { opened: false, reason: 'unclaimed: Opaque tier / a' });
});
