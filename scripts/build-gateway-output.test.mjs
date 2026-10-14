// Caller: build.sh script-guards
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { PassThrough } from 'node:stream';
import test from 'node:test';
import { filterFingerprint, fingerprintSummary, testSummary } from './build-gateway-output.mjs';
import { watchReaderProgress } from './build-gateway-reader-deadline.mjs';

test('large fingerprint streams stay in their artifact while diagnostics pass through', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'fingerprint-output-'));
  try {
    const input = new PassThrough();
    const output = new PassThrough();
    let visible = '';
    output.on('data', chunk => { visible += chunk; });
    const artifact = path.join(directory, 'fingerprint.log');
    const result = filterFingerprint(input, output, artifact);
    const trace = 'INFO cargo::core::compiler::fingerprint: dirty: OpaqueReason(detail)\n'.repeat(10000);
    input.end(`${trace}warning: opaque diagnostic\n`);
    await result;
    assert.equal(visible, 'warning: opaque diagnostic\n');
    assert.equal(fs.readFileSync(artifact, 'utf8'), trace);
    assert.deepEqual(fingerprintSummary(trace), { lines: 10000, dirty_reasons: [['OpaqueReason', 10000]] });
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('test summaries aggregate serial runs without per-unit timing output', () => {
  assert.deepEqual(testSummary('PASS [0.1s] opaque a\nSummary 1 test run\nPASS [0.1s] opaque b\nSummary 1 test run\n'), { runs: 2, last: 'Summary 1 test run' });
});

test('reader failure deadline follows actual write progress and still refuses a stalled reader', () => {
  let callback, bytes = 0, expired = 0;
  const scheduled = new Map();
  let id = 0;
  const stop = watchReaderProgress('opaque.log', 100, () => { expired++; }, {
    size: () => bytes,
    watch: fn => { callback = fn; return { close() {} }; },
    schedule: fn => { scheduled.set(++id, fn); return id; },
    cancel: key => scheduled.delete(key),
  });
  assert.equal(id, 1);
  callback();
  assert.equal(id, 1, 'an unrelated event is not progress');
  bytes = 100;
  callback();
  assert.equal(id, 2);
  assert.equal(scheduled.has(1), false);
  scheduled.get(2)();
  assert.equal(expired, 1);
  stop();
});


test('a failed trace artifact drains the stream and preserves diagnostics', async () => {
  const input = new PassThrough();
  const output = new PassThrough();
  let visible = '';
  output.on('data', chunk => { visible += chunk; });
  const result = filterFingerprint(input, output, path.join(os.tmpdir(), 'missing-trace-directory', 'trace.log'));
  input.end('INFO cargo::core::compiler::fingerprint: dirty: Reason()\nerror: opaque compile failure\n');
  assert.deepEqual(await result, { degraded: true });
  assert.match(visible, /WARNING: fingerprint artifact unavailable/);
  assert.match(visible, /error: opaque compile failure/);
  assert.doesNotMatch(visible, /cargo::core::compiler::fingerprint/);
});
