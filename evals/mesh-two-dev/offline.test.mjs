import test from 'node:test';
import { targets } from './targets.mjs';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { verifyPair } from './identity.mjs';
import { originPlan, driveOrigin, driveBoth, verifyReport } from './origins.mjs';
import { clockBounds, correlate } from './clock.mjs';
import { SseParser, EventOrder, collectState } from './stream.mjs';
import { observeRendered } from './rendered.mjs';
import { verifyConvergence, verifyRestart, coordinateRestart } from './convergence.mjs';
const ids = { aiderqa: 'aaaaaa', windows: 'bbbbbb' };
function pair() {
  return Object.fromEntries(['aiderqa', 'windows'].map((name, i) => [name, {
    status: { instance_id: ids[name], enabled: true, api_port: targets[name].api_port, paired_count: 1 },
    peers: { local_dev: true, paired: [{ instance_id: ids[i ? 'aiderqa' : 'windows'], api_port: targets[i ? 'aiderqa' : 'windows'].api_port, online: true }], incoming_requests: [] },
  }]));
}
test('paired-only verifier refuses production, extra identities, offline and unproven DEV', () => {
  assert.equal(verifyPair(pair(), ids).paired, true);
  for (const mutate of [
    value => value.windows.status.api_port = 7685,
    value => value.aiderqa.peers.paired.push({ instance_id: 'prod', api_port: 7685 }),
    value => value.windows.peers.local_dev = false,
    value => value.aiderqa.peers.paired[0].online = false,
    value => value.aiderqa.peers.paired[0].instance_id = 'foreign',
  ]) { const value = pair(); mutate(value); assert.throws(() => verifyPair(value, ids)); }
});
test('opposite-origin driver qualifies routes and preserves failures before stopping', async () => {
  const writes = [], fixture = { tabId: 'tab', sessionId: 'session', payloadPad: 'probe.md', reportPad: 'report.md', payloadDigest: createHash('sha256').update('opaque-probe').digest('hex') };
  for (const origin of ['aiderqa', 'windows']) {
    const plan = originPlan(origin, ids, fixture);
    assert.ok(plan.every(step => step.args.includes('--mesh') && step.home !== origin));
    const results = await driveOrigin(plan, async step => ({ originName: origin, originPort: origin === 'aiderqa' ? 7697 : 7686, originPeerId: ids[origin], answeredBy: step.targetPeerId, code: 0, pasteId: 'paste-id', body: 'opaque-probe' }), async value => { writes.push(value); return 'evidence.json'; }, new AbortController().signal);
    assert.equal(results.length, 6);
  }
  const plan = originPlan('aiderqa', ids, fixture);
  await assert.rejects(driveOrigin(plan, async () => ({ originName: 'windows', originPort: 7686 }), async value => writes.push(value), new AbortController().signal), /Unproven/);
  assert.equal(writes.at(-1).status, 'FAIL');
});
test('report receipt requires persistence and visibility at the actual origin', () => {
  const expected = { originPeerId: ids.aiderqa, homePeerId: ids.windows, tabId: 'tab', sessionId: 'session', body: 'opaque-report' };
  const receipt = { ...expected, senderPeerId: ids.windows, persisted: true, visible: true };
  assert.equal(verifyReport(receipt, expected), true);
  assert.throws(() => verifyReport({ ...receipt, visible: false }, expected));
  assert.throws(() => verifyReport({ ...receipt, sessionId: 'sibling' }, expected));
});
test('clock correlation uses conservative uncertainty bounds rather than midpoint guesses', () => {
  const offset = clockBounds([{ sentMs: 100, receivedMs: 120, remoteMs: 160 }, { sentMs: 200, receivedMs: 220, remoteMs: 255 }]);
  assert.deepEqual(offset, { lower: 40, upper: 55 });
  const home = { eventId: 'boot:1', tabId: 'tab', sessionId: 'session', state: { status: 'Working' }, atMs: 1000, evidenceKind: 'rendered' };
  assert.deepEqual(correlate(home, { ...home, atMs: 1200 }, offset), { lower: 145, upper: 160 });
  assert.throws(() => correlate(home, { ...home, atMs: 2050 }, offset), /not proven/);
  assert.throws(() => correlate(home, { ...home, eventId: 'boot:2' }, offset));
  assert.throws(() => clockBounds([{ sentMs: 5, receivedMs: 4, remoteMs: 8 }]));
});
test('SSE parser preserves chunked CRLF and multiline frames, detects truncated input', () => {
  const parser = new SseParser();
  assert.deepEqual(parser.push(':keepalive\r\n\r\ndata: {"boot":"b",\r\n'), []);
  assert.deepEqual(parser.push('data: "seq":1}\r\n\r\n'), [{ boot: 'b', seq: 1 }]);
  parser.push('data: incomplete');
  assert.throws(() => parser.finish(), /Truncated/);
});
test('event ordering rejects gaps, duplicates and silent boot changes; overflow requires resync', () => {
  const order = new EventOrder();
  order.accept({ boot: 'b', seq: 1 });
  assert.throws(() => order.accept({ boot: 'b', seq: 1 }), /sequence/);
  assert.throws(() => order.accept({ boot: 'b', seq: 3 }), /sequence/);
  assert.throws(() => order.accept({ boot: 'new', seq: 1 }), /Boot changed/);
  assert.equal(order.accept({ event_type: 'overflow', data: { reason: 'boot-changed' } }).resyncRequired, true);
  order.accept({ boot: 'new', seq: 1 });
});
test('state collector fetches on events and records bad frames before failure', async () => {
  const writes = []; let reads = 0;
  async function* chunks() { yield 'data: {"boot":"b","seq":1,"event_type":"tab-status-changed"}\n\n'; yield 'data: {"boot":"b","seq":3}\n\n'; }
  await assert.rejects(collectState({ chunks: chunks(), fetchState: async () => { reads++; return { tabs: [] }; }, write: async event => writes.push(event), signal: new AbortController().signal, now: () => 10 }), /sequence/);
  assert.equal(reads, 1);
  assert.equal(writes.length, 3);
  assert.equal(writes.at(-1).frame.seq, 3);
});
test('rendered observations are mutation-driven and dispose the observer', () => {
  let callback, disconnected = false; const samples = [];
  class Observer { constructor(fn) { callback = fn; } observe() {} disconnect() { disconnected = true; } }
  const dispose = observeRendered({ root: {}, read: () => ({ status: 'Working', sending: true, progress: 'opaque' }), write: event => samples.push(event), identity: { tabId: 't', sessionId: 's', view: 'web' }, eventId: () => 'b:1', Observer, now: () => 5 });
  callback(); dispose();
  assert.equal(samples.length, 2);
  assert.equal(samples[0].evidenceKind, 'rendered');
  assert.equal(disconnected, true);
});
const baseline = { tabs: [{ id: 't', sessionId: 's', provider: 'p', turnIds: ['1', '2'] }] };
function restart() {
  return { instance: 'windows', oldBoot: 'old', newBoot: 'new', lifecycle: { listenerReady: true, reconnected: true, boot: 'new' }, baseline,
    views: Object.fromEntries(['local', 'remote', 'web'].map(view => [view, { ...structuredClone(baseline), boot: 'new', reconciled: true }])) };
}
test('restart oracle requires changed boot and complete history in all three views', () => {
  assert.equal(verifyRestart(restart()), true);
  for (const mutate of [
    value => value.newBoot = 'old',
    value => value.views.web.tabs[0].turnIds.reverse(),
    value => value.views.remote.tabs.push({ ...value.views.remote.tabs[0] }),
    value => value.views.local.tabs[0].sessionId = 'foreign',
    value => value.views.web.reconciled = false,
  ]) { const value = restart(); mutate(value); assert.throws(() => verifyRestart(value)); }
  assert.equal(verifyConvergence(baseline, { tabs: [] }, ['t']), true);
  assert.throws(() => verifyConvergence(baseline, baseline, ['t']));
});
test('restart coordinator refuses PROD and unknown endpoint authorization', async () => {
  let restarts = 0;
  const adapter = { snapshot: async () => baseline, authorizedUtility: async instance => ({ instance, port: 7685, canonical: true, exactGate: 'restart' }), restart: async () => restarts++ };
  await assert.rejects(coordinateRestart(adapter, 'windows', new AbortController().signal, async () => {}), /authorization/);
  await assert.rejects(coordinateRestart(adapter, 'owner', new AbortController().signal, async () => {}), /named/);
  assert.equal(restarts, 0);
});
test('authorized restart waits for event-backed convergence and preserves exact failure evidence', async () => {
  const written = [];
  const proof = restart();
  const adapter = { snapshot: async () => baseline, authorizedUtility: async instance => ({ instance, port: 7686, canonical: true, exactGate: 'Windows DEV restart coordinated by Fermi', via: 'Euler', owner: 'Fermi' }), requestRestart: async () => {}, restart: async () => { throw new Error('Tester cannot restart Windows directly'); }, waitReconverged: async () => proof };
  assert.equal(await coordinateRestart(adapter, 'windows', new AbortController().signal, async value => written.push(value)), true);
  adapter.waitReconverged = async () => { throw new Error('reconnect deadline'); };
  await assert.rejects(coordinateRestart(adapter, 'windows', new AbortController().signal, async value => written.push(value)), /reconnect deadline/);
  assert.equal(written.at(-1).status, 'FAIL');
  assert.deepEqual(written.at(-1).before, baseline);
});
test('both-origin driver refuses incomplete pairing before invoking a command adapter', async () => {
  let calls = 0;
  const endpoints = pair();
  endpoints.windows.peers.paired = [];
  await assert.rejects(driveBoth({ endpoints, ids, fixtures: {}, adapters: { aiderqa: async () => calls++ }, write: async () => {}, signal: new AbortController().signal }), /exclusively/);
  assert.equal(calls, 0);
});

test('Windows lifecycle refuses wrong authority and missing request route before process control', async () => {
  for (const patch of [{ via: 'other' }, { owner: 'other' }, { requestRestart: undefined }]) {
    let calls = 0;
    const written = [];
    const adapter = {
      snapshot: async () => baseline,
      authorizedUtility: async instance => ({ instance, port: 7686, canonical: true, exactGate: 'Windows DEV restart', via: patch.via ?? 'Euler', owner: patch.owner ?? 'Fermi' }),
      requestRestart: async () => calls++,
      restart: async () => calls++,
    };
    if (Object.hasOwn(patch, 'requestRestart')) adapter.requestRestart = patch.requestRestart;
    await assert.rejects(coordinateRestart(adapter, 'windows', new AbortController().signal, async value => written.push(value)), /Euler to Fermi/);
    assert.equal(calls, 0);
    assert.equal(written.at(-1).status, 'FAIL');
  }
});
