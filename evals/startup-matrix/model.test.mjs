import test from 'node:test';
import assert from 'node:assert/strict';
import { policy, WINDOW_MS, matrix, lockSites, evaluateSla, replyWindow, remainingDeadline } from './model.mjs';
test('General and activity boundary classify independently from observed frozen state', () => {
  const now = WINDOW_MS * 3;
  assert.equal(policy({}, { name: 'General' }, now), 'awake');
  assert.equal(policy({ frozen: true, last_active_at_ms: now - WINDOW_MS }, {}, now), 'awake');
  assert.equal(policy({ last_active_at_ms: now - WINDOW_MS - 1 }, {}, now), 'asleep');
  assert.equal(policy({ last_active_at_ms: null }, {}, now), 'asleep');
});
test('matrix includes fixtures and an outside recent tab without duplication', () => {
  const rows = [{ id: 'a', workspace: 'General' }, { id: 'b', workspace: 'Endure', policy: 'awake' },
    { id: 'c', workspace: 'Other', policy: 'awake' }, { id: 'd', workspace: 'Other', policy: 'asleep' }];
  assert.deepEqual(matrix(rows).map(t => t.id), ['a', 'b', 'c', 'd']);
});
test('lock aggregation excludes 500 ms and retains every slow IPC line', () => {
  const result = lockSites(['[LOCK-HOLD] mux.write() held 500ms at a:1', '[LOCK-HOLD] mux.write() held 501ms at a:1',
    '[LOCK-HOLD] mux.write() held 900ms at a:1', '[SLOW-IPC] command 90ms']);
  assert.equal(result[0].count, 2); assert.equal(result[0].total_ms, 1401); assert.equal(result[1].count, 1);
});
test('wait durations do not hide the lock site and IPC groups across different durations', () => {
  const result = lockSites(['[LOCK-HOLD] mux.write() held 16000ms (waited 0ms) at provider_signals.rs:103',
    '[SLOW-IPC] get_config 1000ms {}', '[SLOW-IPC] get_config 2000ms {}']);
  assert.equal(result[0].site, 'mux.write() at provider_signals.rs:103');
  assert.equal(result[1].site, 'get_config'); assert.equal(result[1].total_ms, 3000);
});
test('owner SLA accepts its boundary and rejects slow or refused sends and missing evidence', () => {
  const chat = { backing: 'chat_backend', accepting_ms: 1000, reply_ok: true };
  assert.equal(evaluateSla(chat).pass, true);
  assert.equal(evaluateSla({ ...chat, accepting_ms: 1001 }).pass, false);
  assert.equal(evaluateSla({ ...chat, send: { code: 1 } }).pass, false);
  const pty = { backing: 'pty', boot: { frozen: true }, wake_ms: 5000, reply_ok: true };
  assert.equal(evaluateSla(pty).pass, true);
  assert.equal(evaluateSla({ ...pty, wake_ms: 5001 }).pass, false);
  assert.equal(evaluateSla({ ...pty, wake_ms: null }).pass, false);
});
test('kernel microsecond timestamps yield integer AbortSignal deadlines', () => {
  assert.equal(remainingDeadline(1000, 800.0390625, 120000), 119801);
  assert.equal(remainingDeadline(130000, 0.0390625, 120000), 1);
});
test('only assistant items after the exact probe and before another user count', () => {
  const messages = [{ type: 'assistant', content: 'PONG' }, { type: 'user', id: 'u', content: 'routed: unique-probe' },
    { type: 'assistant', id: 'a', content: 'PONG' }, { type: 'user', content: 'owner turn' },
    { type: 'assistant', content: 'another answer' }];
  assert.deepEqual(replyWindow(messages, 'unique-probe').following.map(m => m.id), ['a']);
  assert.equal(replyWindow(messages, 'absent'), null);
});
