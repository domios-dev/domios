import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { armBrowserDiagnostics, diagnosticRecord, pendingStreamRequests } from './browser-diagnostics.mjs';

const timestamp = '2026-10-10T11:00:00.000Z';
const request = { requestId: 1, url: 'http://fixture/api/mesh/relay-stream?peer=own&path=%2Fapi%2Foutput%2Fstream&tab=mesh%3Aown%2Ffixture', method: 'GET' };

test('request initiation and terminal events retain one correlated stream identity', () => {
  for (const event of ['request', 'requestfinished', 'requestfailed']) {
    const record = diagnosticRecord(event, { ...request, failure: 'net::ERR_ABORTED' }, timestamp);
    assert.equal(record.event, event);
    assert.equal(record.timestamp, timestamp);
    assert.equal(record.url, request.url);
    assert.equal(record.method, 'GET');
    assert.equal(record.requestId, 1);
    if (event === 'requestfailed') assert.equal(record.failure, 'net::ERR_ABORTED');
  }
  assert.equal(diagnosticRecord('request', { ...request, url: 'http://fixture/api/config' }, timestamp), null);
  assert.equal(diagnosticRecord('request', { ...request, url: 'http://fixture/api/output/stream' }, timestamp).event, 'request');
});

test('pending response headers remain distinct from absent, finished and failed fetches', () => {
  const start = diagnosticRecord('request', request, timestamp);
  const response = diagnosticRecord('response', { ...request, status: 200 }, timestamp);
  assert.deepEqual(pendingStreamRequests([]), []);
  assert.equal(pendingStreamRequests([start, response])[0].responseStatus, 200);
  for (const event of ['requestfinished', 'requestfailed']) {
    assert.deepEqual(pendingStreamRequests([start, response, diagnosticRecord(event, request, timestamp)]), []);
  }
});

test('console levels and page exceptions preserve diagnostic text', () => {
  for (const level of ['log', 'debug', 'info', 'warning', 'error', 'trace', 'assert']) {
    const record = diagnosticRecord('console', { level, text: 'opaque console message', location: { url: 'http://fixture/app.js', lineNumber: 2, columnNumber: 3 } }, timestamp);
    assert.equal(record.level, level);
    assert.equal(record.text, 'opaque console message');
    assert.equal(record.location.lineNumber, 2);
  }
  assert.equal(diagnosticRecord('pageerror', { name: 'TypeError', message: 'opaque error', stack: 'opaque stack' }, timestamp).stack, 'opaque stack');
});

test('gateway credentials are removed from URLs, console text, failures and exception stacks', () => {
  const secret = 'opaque+/secret';
  for (const [event, payload] of [
    ['request', { ...request, url: 'http://fixture/api/output/stream?token=foreign&tab=fixture' }],
    ['console', { level: 'error', text: secret + encodeURIComponent(secret) }],
    ['requestfailed', { ...request, failure: secret }],
    ['pageerror', { message: secret, stack: secret }],
  ]) {
    const text = JSON.stringify(diagnosticRecord(event, payload, timestamp, [secret]));
    assert.ok(!text.includes(secret));
    assert.ok(!text.includes(encodeURIComponent(secret)));
    assert.ok(!text.includes('foreign'));
    assert.ok(text.includes('[REDACTED]'));
  }
});

test('case recorder observes initiation before response and detaches before teardown', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'browser-diagnostics-'));
  try {
    const page = new EventEmitter();
    const recorder = armBrowserDiagnostics(page, directory, { now: () => timestamp });
    const req = { url: () => request.url, method: () => 'GET' };
    page.emit('request', req);
    page.emit('response', { request: () => req, status: () => 200 });
    await recorder.finish('capture-failed');
    const summary = JSON.parse(await readFile(path.join(directory, 'browser-requests.json'), 'utf8'));
    assert.equal(summary.initiated, 1);
    assert.equal(summary.pending.length, 1);
    assert.equal(summary.pending[0].responseStatus, 200);
    for (const event of ['request', 'response', 'requestfinished', 'requestfailed', 'console', 'pageerror']) assert.equal(page.listenerCount(event), 0);
    const rows = (await readFile(path.join(directory, 'browser-events.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
    assert.equal(rows.at(-1).event, 'capture-boundary');
    assert.deepEqual(rows.at(-1).pendingRequestIds, [1]);
  } finally { await rm(directory, { recursive: true, force: true }); }
});
