import test from 'node:test';
import assert from 'node:assert/strict';
import { scanLogAck, observeLogMutation } from './log-ack.mjs';

test('acknowledgement before a large log burst is scanned before retention', () => {
  assert.equal(scanLogAck('', 'ACK' + 'x'.repeat(100000), 'ACK').matched, true);
});

test('acknowledgement split between reads survives without retaining the log', () => {
  const first = scanLogAck('', 'x'.repeat(100000) + 'A', 'ACK');
  assert.equal(first.matched, false);
  assert.ok(first.carry.length <= 2);
  assert.equal(scanLogAck(first.carry, 'CK', 'ACK').matched, true);
});

test('tail position and observer precede mutation and synchronous acknowledgement is drained', async () => {
  const order = []; let text = 'old ACK'; let closed = 0;
  const source = {
    position() { order.push('position'); return text.length; },
    subscribe() { order.push('subscribe'); return () => closed++; },
    readFrom(position) { return { position: text.length, text: text.slice(position) }; },
  };
  const result = await observeLogMutation({ source, marker: 'ACK', signal: new AbortController().signal,
    mutate() { order.push('mutate'); text += 'new ACK'; } });
  assert.deepEqual(order, ['position', 'subscribe', 'mutate']);
  assert.equal(result.position, text.length);
  assert.equal(closed, 1);
});

test('old acknowledgements cannot satisfy a new mutation and cancellation removes the observer', async () => {
  const controller = new AbortController(); let closed = 0;
  const source = { position: () => 7, subscribe: () => () => closed++, readFrom: p => ({ position: p, text: '' }) };
  const result = observeLogMutation({ source, marker: 'ACK', signal: controller.signal, mutate() {} });
  controller.abort(Error('deadline'));
  await assert.rejects(result, /deadline/);
  assert.equal(closed, 1);
});
