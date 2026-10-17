// Caller: node --test scripts/cache-stats.test.mjs
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { summarizeCacheStats, formatCacheStats } from './cache-stats.mjs';

const records = [
  { command: 'nextest api', sccache: { delta: { requests: 8, hits: 4, misses: 2, non_cacheable: 2, not_cached: { incremental: 2 } } } },
  { command: 'nextest core', sccache: { delta: { requests: 3, hits: 2, misses: 1, non_cacheable: 0, not_cached: {} } } },
  { command: 'nextest unavailable', sccache: { delta: null } },
  { command: 'dev', sccache: { delta: { requests: 0, hits: 0, misses: 0, non_cacheable: 0, not_cached: {} } } },
];

test('aggregates retained per-job deltas by command kind and tracks missing evidence', () => {
  assert.deepEqual(summarizeCacheStats(records), [
    { kind: 'dev', jobs: 1, measured_jobs: 1, unmeasured_jobs: 0, requests: 0, hits: 0, misses: 0, non_cacheable: 0, reasons: {}, hit_rate: null },
    { kind: 'nextest', jobs: 3, measured_jobs: 2, unmeasured_jobs: 1, requests: 11, hits: 6, misses: 3, non_cacheable: 2, reasons: { incremental: 2 }, hit_rate: 2 / 3 },
  ]);
});

test('prints non-cacheable reason totals and does not count them as misses', () => {
  const output = formatCacheStats(summarizeCacheStats(records));
  assert.match(output, /nextest 3 2 1 11 6 3 66\.7% 2/);
  assert.match(output, /not cacheable: incremental=2/);
});

test('command reads gateway records from its directory', () => {
  const directory = mkdtempSync(path.join(tmpdir(), 'cache-stats-'));
  try {
    mkdirSync(path.join(directory, 'jobs'));
    writeFileSync(path.join(directory, 'jobs', 'one.json'), JSON.stringify(records[0]));
    const script = fileURLToPath(new URL('./cache-stats.mjs', import.meta.url));
    const result = spawnSync(process.execPath, [script, '--dir', directory], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /nextest 1 1 0 8 4 2 66\.7% 2/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
