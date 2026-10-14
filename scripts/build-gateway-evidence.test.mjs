// Caller: node --test scripts/build-gateway-evidence.test.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { cacheSnapshot, cacheDelta, summarizeDiagnostics, nonCacheableEvent } from './build-gateway-evidence.mjs';

const fixture = JSON.parse(fs.readFileSync(new URL('./fixtures/build-gateway-evidence.json', import.meta.url)));
const endpoint = { host: '127.0.0.1', port: 4226 };

test('cache delta sums language counters and uses hits plus misses as rate denominator', () => {
  const start = cacheSnapshot(fixture.start, endpoint);
  const end = cacheSnapshot(fixture.end, endpoint);
  assert.deepEqual(cacheDelta(start, end), { requests: 20, hits: 12, misses: 5, non_cacheable: 3, not_cached: { 'crate-type': 2, incremental: 1 }, hit_rate: 12 / 17 });
  assert.equal(cacheDelta(start, start).hit_rate, null);
});

test('unavailable, reset and changed-server counters cannot be presented as a cache measurement', () => {
  const start = cacheSnapshot(fixture.start, endpoint);
  const end = cacheSnapshot(fixture.end, endpoint);
  assert.equal(cacheDelta(null, end), null);
  assert.equal(cacheDelta(end, start), null);
  assert.equal(cacheDelta(start, end, true), null);
  assert.equal(cacheDelta(start, { ...end, location: 'local' }), null);
  assert.equal(cacheDelta(start, { ...end, endpoint: { port: 4227 } }), null);
  assert.equal(cacheDelta(start, { ...end, not_cached: {} }), null);
  assert.throws(() => cacheSnapshot({ stats: {} }));
});

test('fixture reasons attach to their own unit across interleaved Cargo scopes', () => {
  const original = structuredClone(fixture);
  const summary = summarizeDiagnostics(fixture.fingerprint, fixture.sccache);
  assert.equal(summary.rebuilt_units.length, 2);
  assert.match(summary.rebuilt_units[0].reasons[0], /cfg_if/);
  assert.match(summary.rebuilt_units[1].reasons[0], /failed to read/);
  assert.deepEqual(summary.non_cacheable_units, [
    { unit: 'getrandom/build-script-build', crate_type: 'bin', reason: 'crate-type, bin' },
    { unit: 'serde_derive', crate_type: 'proc-macro', reason: 'crate-type, proc-macro' },
  ]);
  assert.equal(nonCacheableEvent('parse_arguments: CannotCache(foo): [invalid]'), null);
  assert.deepEqual(fixture, original);
});

test('state lifecycle persists scope and compact evidence beside the original job and logs', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-evidence-'));
  const script = new URL('./build-gateway-state.mjs', import.meta.url);
  try {
    const log = path.join(directory, 'jobs', 'fixture.log');
    const run = (command, args = []) => {
      const result = spawnSync(process.execPath, [fileURLToPath(script), command, '--dir', directory, '--job-id', 'fixture', ...args], { encoding: 'utf8', env: { ...process.env, SCCACHE_ERROR_LOG: path.join(directory, 'sccache-server.log') } });
      assert.equal(result.status, 0, result.stderr);
    };
    run('create', ['--job', 'check-queue', '--crates', 'codeterm-plan codeterm', '--touched-files-count', '3', '--log-path', log]);
    fs.writeFileSync(log, fixture.fingerprint.join('\n'));
    fs.writeFileSync(path.join(directory, 'sccache-server.log'), 'previous job\n');
    run('evidence-start', ['--scope', 'fixture']);
    fs.appendFileSync(path.join(directory, 'sccache-server.log'), `${fixture.sccache.join('\n')}\n`);
    run('evidence-end');
    run('update', ['--state', 'completed', '--exit-code', '0']);
    const record = JSON.parse(fs.readFileSync(path.join(directory, 'jobs', 'fixture.json')));
    assert.deepEqual(record.crates, ['codeterm-plan', 'codeterm']);
    assert.equal(record.touched_files_count, 3);
    assert.equal(record.state, 'completed');
    assert.equal(record.diagnostics.rebuilt_units.length, 2);
    assert.equal(record.diagnostics.non_cacheable_units.length, 2);
    assert.match(fs.readFileSync(record.fingerprint_log_path, 'utf8'), /getrandom/);
    assert.doesNotMatch(fs.readFileSync(record.diagnostics.sccache_log_path, 'utf8'), /previous job/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
