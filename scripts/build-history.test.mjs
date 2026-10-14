// Caller: node --test scripts/build-history.test.mjs
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { summarizeHistory, median, formatHistory, parseHistoryArgs } from './build-history.mjs';

const jobs = JSON.parse(readFileSync(new URL('./fixtures/build-history.json', import.meta.url)));
const now = Date.parse('2026-10-14T14:00:30Z');

test('fixture history excludes failures from stats and shows elapsed running time with deterministic labels', () => {
  const original = structuredClone(jobs);
  const history = summarizeHistory(jobs, { now, canonical: 'c:\\repo' });
  assert.deepEqual(history.jobs.map(job => job.job_id), ['queued', 'running', 'failed', 'recent', 'middle', 'old', 'lost']);
  assert.equal(history.jobs[1].duration_ms, 30_000);
  assert.equal(history.jobs[0].duration_ms, null);
  assert.equal(history.jobs[1].worktree_label, 'check-worktree');
  assert.equal(history.jobs[2].worktree_label, 'canonical');
  assert.equal(history.jobs[3].worktree_label, 'gauge');
  const check = history.verbs.find(row => row.verb === 'check');
  assert.deepEqual([check.n, check.median_ms, check.p90_ms, check.max_ms], [3, 4000, 9000, 9000]);
  assert.equal(check.trend.delta_ms, null);
  assert.equal(history.verbs.find(row => row.verb === 'build').n, 0);
  assert.deepEqual(jobs, original);
  assert.match(formatHistory(history), /30\.0s elapsed\s+running/);
});

test('last applies after verb filtering and limits the same sample used by stats', () => {
  const history = summarizeHistory(jobs, { now, last: 4, verb: 'check' });
  assert.deepEqual(history.jobs.map(job => job.job_id), ['queued', 'failed', 'recent', 'middle']);
  assert.equal(history.verbs[0].n, 2);
  assert.equal(history.verbs[0].median_ms, 6500);
});

test('trend compares complete successful 10-job windows, even when failures intervene', () => {
  const records = Array.from({ length: 20 }, (_, i) => ({ ...jobs[0], job_id: `j${i}`, started_at: new Date(now - i * 100_000).toISOString(), ended_at: new Date(now - i * 100_000 + (i < 10 ? 2000 : 1000)).toISOString() }));
  records.push({ ...records[0], job_id: 'failed', state: 'failed', exit_code: 1 });
  const trend = summarizeHistory(records, { now }).verbs[0].trend;
  assert.deepEqual(trend, { last_n: 10, previous_n: 10, last_median_ms: 2000, previous_median_ms: 1000, delta_ms: 1000, percent: 100 });
  assert.equal(median([2, 4]), 3);
  assert.equal(median([]), null);
});

test('invalid clocks, limits and durations do not produce misleading stats', () => {
  for (const last of [0, -1, 1.5, NaN, '50']) assert.throws(() => summarizeHistory(jobs, { now, last }));
  assert.throws(() => summarizeHistory(jobs));
  const history = summarizeHistory([{ ...jobs[0], ended_at: 'invalid' }, { ...jobs[1], ended_at: '2000-01-01' }], { now });
  assert.equal(history.verbs[0].n, 0);
  assert.deepEqual(summarizeHistory([], { now }).jobs, []);
  for (const args of [['--last'], ['--verb'], ['--bogus']]) assert.throws(() => parseHistoryArgs(args));
  assert.deepEqual(parseHistoryArgs(['--last', '3', '--verb', 'check', '--json']), { last: 3, verb: 'check', json: true });
});

test('CLI JSON remains parseable with an unreadable record; table exposes cache rate and diagnostics', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'build-history-'));
  try {
    mkdirSync(path.join(root, 'jobs'));
    writeFileSync(path.join(root, 'jobs', 'broken.json'), '{');
    const record = { ...jobs[2], sccache: { delta: { hits: 3, misses: 1, hit_rate: 0.75, non_cacheable: 1, not_cached: { 'crate-type': 1 } } }, diagnostics: { rebuilt_units: [{ unit: 'getrandom', count: 1, reasons: ['dependency changed'] }], non_cacheable_units: [{ unit: 'getrandom/build-script-build', reason: 'crate-type, bin' }] } };
    writeFileSync(path.join(root, 'jobs', 'valid.json'), JSON.stringify(record));
    const result = spawnSync(process.execPath, ['scripts/build-history.mjs', '--dir', root, '--json'], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const history = JSON.parse(result.stdout);
    assert.equal(history.jobs.length, 1);
    assert.match(result.stderr, /broken.json/);
    const text = formatHistory(history);
    assert.match(text, /75\.0% \(3\/4\)/);
    assert.match(text, /getrandom.*dependency changed/);
    assert.match(text, /non-cacheable getrandom\/build-script-build: crate-type, bin/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
