import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { failureAnnotation, failureDetector, failureEvent, firstFailure } from '../ci-first-failure.mjs';
import { captureCommand } from '../ci-rust-tests.mjs';

const env = {
  CI_CANCEL_RUN: 'true', GITHUB_REPOSITORY: 'owner/repo', GITHUB_RUN_ID: '123',
  GITHUB_RUN_ATTEMPT: '2', GITHUB_JOB: 'rust', GITHUB_SHA: 'a'.repeat(40),
  DOMIOS_CI_WEBHOOK_URL: 'http://100.64.0.1:7685/api/ci/webhook', DOMIOS_CI_WEBHOOK_TOKEN: 'opaque',
};

test('fragmented ANSI and UTF-8 lines detect FAIL and TIMEOUT without matching diagnostics', () => {
  const events = [];
  const detector = failureDetector(event => events.push(event));
  const bytes = Buffer.from('stdout FAIL [1s] not_a_result\n  \x1b[31mFAIL\x1b[0m [ 0.02s] binary café::case\r\nTIMEOUT [ 60s] binary slow');
  for (const byte of bytes) detector.write(Buffer.from([byte]));
  detector.end();
  assert.deepEqual(events, [{ test: 'binary café::case', exit: 1 }, { test: 'binary slow', exit: 124 }]);
});

test('deduplication scopes exact test identity by run, attempt and job across streams', () => {
  const seen = new Set();
  const events = [];
  const line = 'FAIL [1s] binary test\n';
  for (const identity of [{ run: '1', attempt: '1', job: 'a' }, { run: '1', attempt: '1', job: 'a' },
    { run: '1', attempt: '2', job: 'a' }, { run: '2', attempt: '1', job: 'a' }, { run: '1', attempt: '1', job: 'b' }]) {
    const detector = failureDetector(event => events.push(event), identity, seen);
    detector.write(line.repeat(2));
    detector.end();
  }
  assert.equal(events.length, 4);
});

test('failure annotation preserves the cause and escapes workflow command delimiters', () => {
  const annotation = failureAnnotation(failureEvent(env, { test: 'binary test\n::warning::injected%value' }));
  assert.equal(annotation.split('\n').length, 2);
  assert.match(annotation, /binary test%0A::warning::injected%25value/);
  assert.match(annotation, /run 123 attempt 2/);
});

test('notify precedes cancelling exactly the current run, even when notification fails', async () => {
  for (const delivery of ['ok', 'refused', 'offline']) {
    const calls = [];
    await firstFailure({ test: 'binary test', exit: 1 }, {
      env, claim: () => true, warn: () => {}, announce: () => calls.push('annotation'),
      fetchEvent: async (url, options) => {
        calls.push('notify');
        assert.equal(url, env.DOMIOS_CI_WEBHOOK_URL);
        assert.equal(options.headers.Authorization, 'Bearer opaque');
        assert.deepEqual(JSON.parse(options.body), failureEvent(env, { test: 'binary test', exit: 1 }));
        assert.equal(options.redirect, 'error');
        assert.ok(options.signal instanceof AbortSignal);
        if (delivery === 'offline') throw new Error('offline');
        return { ok: delivery === 'ok' };
      },
      cancel: async actual => { assert.equal(actual, env); calls.push('cancel'); },
    });
    assert.deepEqual(calls, ['annotation', 'notify', 'cancel']);
  }
});

test('job claim deduplicates streaming and step fallback, with attempts isolated', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ci-failure-'));
  const calls = [];
  const options = { env: { ...env, RUNNER_TEMP: directory },
    fetchEvent: async () => { calls.push('notify'); return { ok: true }; },
    cancel: async () => { calls.push('cancel'); }, warn: () => {}, announce: () => {} };
  try {
    await firstFailure({ test: 'binary one', exit: 1 }, options);
    await firstFailure({ step: 'fallback' }, options);
    await firstFailure({ test: 'binary two', exit: 1 }, options);
    assert.deepEqual(calls, ['notify', 'cancel']);
    await firstFailure({}, { ...options, env: { ...options.env, GITHUB_RUN_ATTEMPT: '3' } });
    assert.deepEqual(calls, ['notify', 'cancel', 'notify', 'cancel']);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('missing secrets and cancellation errors preserve failure; ordinary runs do not cancel', async () => {
  const calls = [];
  const options = { env: { ...env, DOMIOS_CI_WEBHOOK_TOKEN: '' }, claim: () => true,
    fetchEvent: async () => { throw new Error('must not fetch'); },
    cancel: async () => { calls.push('cancel'); throw new Error('unavailable'); }, warn: () => {}, announce: () => {} };
  await assert.doesNotReject(firstFailure({}, options));
  await firstFailure({}, { ...options, env: { ...env, CI_CANCEL_RUN: 'false' } });
  assert.deepEqual(calls, ['cancel']);
});

test('capture fires once while its child is still running and preserves raw exit', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ci-stream-'));
  const ack = path.join(directory, 'ack');
  let calls = 0;
  const child = `
    const fs = require('node:fs');
    const timer = setTimeout(() => process.exit(99), 3000);
    const watcher = fs.watch(${JSON.stringify(directory)}, () => {
      if (fs.existsSync(${JSON.stringify(ack)})) { watcher.close(); clearTimeout(timer); process.exit(42); }
    });
    process.stdout.write('  FAIL [0.1s] binary opaque\\n');
    process.stderr.write('  FAIL [0.1s] binary opaque\\n');
  `;
  try {
    const result = await captureCommand(process.execPath, ['-e', child], path.join(directory, 'log'), {}, false,
      event => { calls++; assert.equal(event.test, 'binary opaque'); fs.writeFileSync(ack, 'observed'); });
    assert.equal(result.status, 42);
    assert.equal(calls, 1);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
