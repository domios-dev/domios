import test from 'node:test';
import assert from 'node:assert/strict';
import { summarizeRecords, proveMediumChat } from './provider-records.mjs';
import { devStreamClient } from './http-stream.mjs';
test('provider summary selects own user records and chronological model evidence', () => {
  const binding = { provider: 'codex', tabId: 'tab', sessionId: 'own' };
  const rows = [{ type: 'session_meta', payload: { id: 'own' } }, { type: 'turn_context', payload: { model: 'cheap-a' } },
    ...Array.from({ length: 15 }, (_, i) => ({ type: 'response_item', timestamp: `time-${i}`, payload: { type: 'message', role: 'user' } })),
    { type: 'turn_context', payload: { model: 'cheap-b' } }];
  const summary = summarizeRecords(rows, binding);
  assert.equal(proveMediumChat(summary, ['cheap-a', 'cheap-b']).turns.length, 15);
  assert.throws(() => summarizeRecords(rows, { ...binding, sessionId: 'foreign' }));
  assert.throws(() => proveMediumChat(summary, ['cheap-b', 'cheap-a']));
  assert.throws(() => proveMediumChat({ ...summary, turns: [] }, ['cheap-a', 'cheap-b']));
});
test('Claude sidechains and OpenCode parts do not inflate chat turn counts', () => {
  const claude = summarizeRecords([{ sessionId: 'own', uuid: 'u1', type: 'user' }, { sessionId: 'own', uuid: 'u2', type: 'user', isSidechain: true }], { provider: 'claude', tabId: 't', sessionId: 'own' });
  assert.equal(claude.turns.length, 1);
  const rows = [{ table: 'session', row: { id: 'own' } }, { table: 'message', row: { id: 'm', session_id: 'own', data: '{"role":"user","model":{"modelID":"cheap"}}' } }, { table: 'part', row: { id: 'p', session_id: 'own', data: '{}' } }];
  const summary = summarizeRecords(rows, { provider: 'opencode', tabId: 't', sessionId: 'own' });
  assert.equal(summary.turns.length, 1);
  assert.equal(summary.models[0].model, 'cheap');
  assert.throws(() => summarizeRecords([...rows, rows[1]], { provider: 'opencode', tabId: 't', sessionId: 'own' }), /Duplicate/);
});
test('unknown providers require explicit record schema; no guessed Grok evidence', () => {
  const binding = { provider: 'grok', tabId: 't', sessionId: 'own' }, rows = [{ sid: 'own', role: 'user', id: 'u', model: 'cheap' }];
  assert.throws(() => summarizeRecords(rows, binding), /schema/);
  assert.equal(summarizeRecords(rows, binding, { sessionPath: ['sid'], rolePath: ['role'], idPath: ['id'], modelPath: ['model'] }).turns.length, 1);
});
test('HTTP SSE adapter stays on aiderqa gateway and rejects target relay refusal', async () => {
  assert.throws(() => devStreamClient({ port: 7685, token: 'opaque' }));
  const calls = [], signal = new AbortController().signal;
  const client = devStreamClient({ port: 7697, token: 'opaque', peer: 'bbbbbb', fetcher: async (url, options) => {
    calls.push({ url, options });
    return { ok: true, json: async () => ({ ok: true, status: 200, data: { tabs: [] } }), body: 'stream' };
  } });
  assert.equal(await client.open(signal), 'stream');
  assert.deepEqual(await client.fetchState(signal), { tabs: [] });
  assert.ok(calls.every(call => call.url.startsWith('http://127.0.0.1:7697/')));
  assert.equal(JSON.parse(calls[1].options.body).target_peer_id, 'bbbbbb');
  const denied = devStreamClient({ port: 7697, token: 'opaque', peer: 'bbbbbb', fetcher: async () => ({ ok: true, json: async () => ({ ok: false, status: 403, data: {} }) }) });
  await assert.rejects(denied.fetchState(signal), /refused/);
  const incomplete = devStreamClient({ port: 7697, token: 'opaque', peer: 'bbbbbb', fetcher: async () => ({ ok: true, json: async () => ({ ok: true, data: {} }) }) });
  await assert.rejects(incomplete.fetchState(signal), /refused/);
});
