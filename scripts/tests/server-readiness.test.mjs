import test from 'node:test';
import assert from 'node:assert/strict';
import { healthReady, waitForReady } from '../server-readiness.mjs';

const event = state => `event: startup-state\r\ndata: ${JSON.stringify({ state })}\r\n\r\n`;
const streamResponse = stream => new Response(stream, { headers: { 'content-type': 'text/event-stream' } });
const closedEvents = text => streamResponse(new ReadableStream({ start(controller) {
  controller.enqueue(new TextEncoder().encode(text)); controller.close();
} }));

test('health readiness requires both healthy status and explicit completed startup', () => {
  assert.equal(healthReady({ status: 'ok', startup: { state: 'ready' } }), true);
  for (const health of [null, {}, { status: 'ok' }, { status: 'ok', startup: { state: 'starting' } },
    { status: 'ok', startup: { state: 'failed' } }, { status: 'error', startup: { state: 'ready' } }]) {
    assert.equal(healthReady(health), false);
  }
});

test('startup waits for events across listener replacement before proving health', async () => {
  const calls = [];
  let deliver;
  const pending = streamResponse(new ReadableStream({ start(controller) { deliver = controller; } }));
  const complete = Promise.withResolvers();
  const wait = waitForReady({ origin: 'http://127.0.0.1:7786', signal: new AbortController().signal,
    token: 'fixture-secret', fetcher: async (url, options) => {
      calls.push(url.pathname);
      assert.equal(options.headers.Authorization, 'Bearer fixture-secret');
      if (calls.length === 1) return closedEvents(event('starting'));
      if (calls.length === 2) { complete.resolve(); return pending; }
      return Response.json({ status: 'ok', startup: { state: 'ready' } });
    } });
  await complete.promise;
  assert.deepEqual(calls, ['/api/startup/events', '/api/startup/events']);
  deliver.enqueue(new TextEncoder().encode(event('ready')));
  await wait;
  assert.deepEqual(calls, ['/api/startup/events', '/api/startup/events', '/api/health']);
});

test('startup refuses terminal failure, incompatible endpoints and dishonest health', async () => {
  for (const responses of [
    [closedEvents(event('failed'))], [new Response('', { status: 404 })], [new Response('<html>')],
    [closedEvents(event('ready')), Response.json({ status: 'ok', startup: { state: 'starting' } })],
  ]) {
    await assert.rejects(waitForReady({ origin: 'http://127.0.0.1:7786', signal: new AbortController().signal,
      fetcher: async () => { assert.ok(responses.length); return responses.shift(); } }));
    assert.equal(responses.length, 0);
  }
});

test('startup abort between listener generations never opens another connection', async () => {
  const controller = new AbortController();
  let requests = 0;
  const response = streamResponse(new ReadableStream({ pull(stream) { controller.abort(); stream.close(); } }));
  await assert.rejects(waitForReady({ origin: 'http://127.0.0.1:7786', signal: controller.signal,
    fetcher: async () => { requests++; return response; } }));
  assert.ok(requests <= 1);
});
