import assert from 'node:assert/strict';
import { test } from 'node:test';
import { matchesOutputRequest, outputReadinessState, waitOutputReadiness, installOutputParseProbe } from './output-readiness.mjs';

const target = { origin: 'http://gateway', peerId: 'own-peer', homePeerId: 'own-peer', tabId: 'fixture' };
const url = 'http://gateway/api/mesh/relay-stream?peer=own-peer&path=%2Fapi%2Foutput%2Fstream&tab=mesh%3Aown-peer%2Ffixture';
const initiated = { event: 'request', requestId: 1, url, timestamp: 'start' };
const headers = { event: 'response', requestId: 1, url, status: 200, timestamp: 'headers' };
const parsed = { event: 'write-parsed', tabId: 'fixture', nonEmpty: true, timestamp: 'parsed' };
const empty = { tab_id: 'fixture', answered_by: { instance_id: 'own-peer' }, text: ' \n' };

function bus(rows = []) {
  const callbacks = new Set();
  return { rows, callbacks, records: () => rows,
    subscribe: fn => { callbacks.add(fn); return () => callbacks.delete(fn); },
    emit: row => { rows.push(row); for (const fn of [...callbacks]) fn(); } };
}

test('only exact peer, origin, composite tab and output path match', () => {
  assert.ok(matchesOutputRequest(url, target));
  for (const other of [url.replace('own-peer&', 'foreign&'), url.replace('fixture', 'other'),
    url.replace('http://gateway', 'http://foreign'), url.replace('output', 'events')]) assert.equal(matchesOutputRequest(other, target), false);
  const local = { ...target, peerId: null };
  const cursor = Buffer.from(JSON.stringify({ tabs: { fixture: 0 } })).toString('base64url');
  assert.ok(matchesOutputRequest('http://gateway/api/output/stream?since=' + cursor, local));
  const aggregate = Buffer.from(JSON.stringify({ tabs: { fixture: 0, other: 0 } })).toString('base64url');
  assert.equal(matchesOutputRequest('http://gateway/api/output/stream?since=' + aggregate, local), false);
});

test('headers followed by first non-empty parse resolve from events, with waiter cleanup', async () => {
  const source = bus();
  const controller = new AbortController();
  const wait = waitOutputReadiness({ ...source, target, signal: controller.signal });
  source.emit(initiated);
  source.emit(headers);
  assert.equal(outputReadinessState(source.rows, target).ready, false);
  source.emit({ ...parsed, nonEmpty: false });
  assert.equal(outputReadinessState(source.rows, target).ready, false);
  source.emit(parsed);
  assert.equal((await wait).ready, true);
  assert.equal(source.callbacks.size, 0);
});

test('queued events resolve without a new event; earlier parse cannot qualify later headers', async () => {
  const source = bus([initiated, headers, parsed]);
  const wait = waitOutputReadiness({ ...source, target, signal: new AbortController().signal });
  assert.equal((await wait).ready, true);
  assert.equal(source.callbacks.size, 0);
  assert.equal(outputReadinessState([initiated, parsed, headers], target).ready, false);
  assert.equal(outputReadinessState([initiated, { ...headers, status: 400 }, parsed], target).ready, false);
  assert.equal(outputReadinessState([initiated, headers, { event: 'requestfailed', requestId: 1, url }, parsed], target).ready, false);
});

test('only explicit same-home and same-tab empty proof substitutes for parse after headers', () => {
  assert.equal(outputReadinessState([initiated, headers], target, empty).ready, true);
  assert.equal(outputReadinessState([initiated], target, empty).ready, false);
  for (const proof of [undefined, { ...empty, text: 'opaque content' }, { ...empty, tab_id: 'other' },
    { ...empty, answered_by: { instance_id: 'foreign' } }]) assert.equal(outputReadinessState([initiated, headers], target, proof).ready, false);
});

test('one deadline saves initiated, header and byte state at abort, releases waiter', async () => {
  const source = bus([initiated, headers,
    { event: 'network-request', networkRequestId: 'wire', url, timestamp: 'start' },
    { event: 'network-response', networkRequestId: 'wire', url, timestamp: 'headers', status: 200 },
    { event: 'network-bytes', networkRequestId: 'wire', url, dataLength: 7, encodedDataLength: 9 }]);
  const controller = new AbortController();
  const wait = waitOutputReadiness({ ...source, target, signal: controller.signal });
  controller.abort();
  await assert.rejects(wait, error => {
    assert.equal(error.message, 'FAIL no output within 30 s');
    assert.equal(error.outputState.requests[0].headers.status, 200);
    assert.equal(error.outputState.network[0].receivedBytes, 7);
    assert.equal(error.outputState.network[0].encodedBytes, 9);
    assert.equal(error.outputState.ready, false);
    return true;
  });
  assert.equal(source.callbacks.size, 0);
});

test('expired signal fails even if queued records look ready', async () => {
  const source = bus([initiated, headers, parsed]);
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(waitOutputReadiness({ ...source, target, signal: controller.signal }), /no output within 30 s/);
  assert.equal(source.callbacks.size, 0);
});

test('init probe registers real write-parsed listener at hook assignment, without guessed initial events', async () => {
  const prior = { document: globalThis.document, window: globalThis.window, MutationObserver: globalThis.MutationObserver };
  let listener;
  let disposed = 0;
  const rows = [];
  const host = {};
  const root = { dataset: { tabId: 'fixture' }, querySelector: () => host };
  globalThis.document = { querySelectorAll: () => [root] };
  globalThis.window = { __b3OutputParsed: row => { rows.push(row); return Promise.resolve(); } };
  globalThis.MutationObserver = class { observe() {} disconnect() { disposed++; } };
  try {
    installOutputParseProbe({ tabId: 'fixture' });
    host.__codetermTerminal = { onWriteParsed: fn => { listener = fn; return { dispose: () => disposed++ }; },
      buffer: { active: { length: 1, type: 'normal', getLine: () => ({ translateToString: () => 'opaque output' }) } } };
    assert.equal(rows.length, 0);
    listener();
    assert.equal(rows[0].nonEmpty, true);
    assert.equal(rows[0].tabId, 'fixture');
    globalThis.window.__b3OutputParseProbe.dispose();
    assert.equal(disposed, 2);
  } finally { Object.assign(globalThis, prior); }
});
