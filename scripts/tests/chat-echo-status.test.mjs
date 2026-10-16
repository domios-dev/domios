import { test } from 'node:test';
import assert from 'node:assert/strict';
import { claimedRowsComplete, observeSendClaims } from '../../evals/chat-echo/observe-status.mjs';

const row = (id, record = id) => ({ tab_id: 'owned', send_id: id, state: 'delivered', matched_row: { record } });
const event = data => ({ event_type: 'chat-send-status', data });

test('status payloads complete claims with only initial and final ledger reads', async () => {
  let reads = 0;
  const queued = [
    { event_type: 'mux-state-changed', data: { tab_id: 'owned' } },
    event({ ...row('a'), tab_id: 'foreign' }), event(row('other')),
    event(row('a')), event(row('b')),
  ];
  const persisted = [];
  const receipt = await observeSendClaims({ tabId: 'owned', sendIds: ['a', 'b'],
    signal: new AbortController().signal, events: { next: async () => queued.shift() },
    readLedger: async () => ++reads === 1 ? [] : [row('a'), row('b')],
    persist: value => persisted.push(structuredClone(value)),
  });
  assert.equal(reads, 2);
  assert.equal(receipt.status, 'completed');
  assert.equal(receipt.events.length, 2);
  assert.deepEqual(persisted.at(-1).finalLedger, [row('a'), row('b')]);
});

test('deadline still preserves a final ledger using an independent live signal', async () => {
  const controller = new AbortController();
  const signals = [];
  const receipt = await observeSendClaims({ tabId: 'owned', sendIds: ['a', 'b'], signal: controller.signal,
    events: { next: async () => { controller.abort(); throw Error('deadline'); } },
    readLedger: async signal => { signals.push(signal); return [row('a')]; }, persist: () => {},
  });
  assert.equal(signals.length, 2);
  assert.notEqual(signals[0], signals[1]);
  assert.equal(signals[1].aborted, false);
  assert.equal(receipt.status, 'incomplete');
  assert.match(receipt.error, /deadline/);
  assert.deepEqual(receipt.finalLedger, [row('a')]);
});

test('final ledger failure is retained and duplicate source claims never pass', async () => {
  assert.equal(claimedRowsComplete([row('a', 'same'), row('b', 'same')], ['a', 'b']), false);
  assert.equal(claimedRowsComplete([row('a')], ['a', 'a']), false);
  let reads = 0;
  const receipt = await observeSendClaims({ tabId: 'owned', sendIds: ['a'],
    signal: new AbortController().signal, events: { next: async () => { throw Error('stream ended'); } },
    readLedger: async () => { if (++reads === 2) throw Error('daemon unavailable'); return []; }, persist: () => {},
  });
  assert.equal(receipt.status, 'incomplete');
  assert.match(receipt.error, /stream ended/);
  assert.match(receipt.finalLedgerError, /daemon unavailable/);
});
