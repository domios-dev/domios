import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { EventEmitter } from 'node:events';
import { assetIdentity, safeUrl, phaseBreakdown, logMarks, recoveryVerdict, userIds, diagnosticVerdict, tabAuthority } from '../../evals/startup-matrix/evidence-model.mjs';
import { WireCapture, snapshotOutputEvidence } from '../../evals/startup-matrix/output-evidence.mjs';
import { processExit, settleRecovery } from '../../evals/startup-matrix/recovery-settlement.mjs';
import { evidenceVerdict, verifyByteArtifacts } from '../../evals/startup-matrix/evidence-contract.mjs';
import { loopback, options, assertSameListener } from '../../evals/startup-matrix/evidence-runtime.mjs';
import { captureBounded } from '../../evals/startup-matrix/phase-collector.mjs';

const fixture = JSON.parse(readFileSync(new URL('../../tests/fixtures/startup-evidence.json', import.meta.url)));

test('bundle identity hashes actual content independently of URL and strips credentials', () => {
  const loaded = assetIdentity('http://user:secret@127.0.0.1:7686/a.js?token=secret#private', Buffer.from('abc'), 'CDP');
  assert.deepEqual(loaded, { url: 'http://127.0.0.1:7686/a.js', bytes: 3, origin: 'CDP',
    sha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' });
  assert.notEqual(assetIdentity(loaded.url, Buffer.from('abd'), 'HTTP').sha256, loaded.sha256);
  assert.equal(safeUrl('http://127.0.0.1/a?token=x'), 'http://127.0.0.1/a');
});

test('phase origins retain units and absent values without inventing zero-duration phases', () => {
  const [row] = phaseBreakdown(fixture.phases, fixture.state);
  assert.equal(row.socketQueueMs, 1);
  assert.equal(row.creationToReadyMs, 6);
  assert.equal(row.origins.cli_begin.unit, 'epoch_ms');
  assert.equal(row.origins.socket_bound.source, 'boot-request bound_at_us');
  assert.deepEqual(row.missing, []);
  const [unknown] = phaseBreakdown({ rows: [{ id: 'absent' }], marks: fixture.phases.marks }, fixture.state);
  assert.equal(unknown.creationToReadyMs, null);
  assert.equal(unknown.origins.utility_created_at.value, null);
  assert.equal(unknown.missing.length, 7);
});

test('raw phase window captures request bind and excludes unrelated time and PTY', () => {
  const raw = '[boot-request] tab=own request_id=1 method=spawn received_at_us=10 dispatched_at_us=11 bound_at_us=12\n' +
    '[boot-phase] tab=own pty=p1 phase="AgentReady" at_us=15\n' +
    '[boot-phase] tab=other pty=p2 phase="AgentReady" at_us=19\n';
  assert.deepEqual(logMarks(raw, 10, 18).map(mark => [mark.phase, mark.atUs]), [['request_bound', 12], ['AgentReady', 15]]);
  assert.throws(() => logMarks(raw, undefined, 18));
});

test('wire preserves bytes and stream identity including live data arriving before buffered CDP reply', () => {
  const wire = new WireCapture(100);
  wire.arm('one');
  wire.add('one', Buffer.from('data: tail\r\n\r\n').toString('base64'));
  wire.add('two', Buffer.from('data: other\n\n').toString('base64'));
  wire.begin('one', Buffer.from('data: head\r\n\r\n').toString('base64'));
  assert.equal(wire.records().find(stream => stream.id === 'one').bytes.toString(), 'data: head\r\n\r\ndata: tail\r\n\r\n');
  assert.equal(wire.records().find(stream => stream.id === 'two').bytes.toString(), 'data: other\n\n');
  assert.throws(() => wire.add('one', Buffer.alloc(101).toString('base64')), /bound/);
});

test('snapshot captures hidden and visible terminals and all three buffers with own tab identity', () => {
  const buffer = (type, content) => ({ type, length: 1, baseY: 0, viewportY: 0, getLine: () => ({ translateToString: () => content }) });
  const element = (id, hidden) => ({ __codetermTerminal: { cols: 80, rows: 24, buffer: {
    active: buffer('active', `${id}-active`), normal: buffer('normal', `${id}-normal`), alternate: buffer('alternate', `${id}-alternate`),
  } }, tagName: 'DIV', hidden, getAttribute: name => name === 'data-tab-id' ? id : null,
  getBoundingClientRect: () => ({ toJSON: () => ({ width: hidden ? 0 : 80 }) }), closest() { return this; } });
  const prior = Object.fromEntries(['window', 'document', 'getComputedStyle'].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  try {
    globalThis.window = {};
    globalThis.document = { visibilityState: 'visible', scripts: [], querySelectorAll: () => [element('visible', false), element('hidden', true)] };
    globalThis.getComputedStyle = element => ({ display: element.hidden ? 'none' : 'block', visibility: 'visible', opacity: '1' });
    const snapshot = snapshotOutputEvidence();
    assert.deepEqual(snapshot.terminals.map(terminal => terminal.tabId), ['visible', 'hidden']);
    assert.equal(snapshot.terminals[1].normal.text, 'hidden-normal');
    assert.equal(snapshot.terminals[1].alternate.text, 'hidden-alternate');
    assert.equal(snapshot.terminals[1].ancestors[0].display, 'none');
    assert.equal(snapshot.terminals[0].active.text, 'visible-active');
  } finally {
    for (const [key, descriptor] of Object.entries(prior)) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key];
    }
  }
});

test('natural shipped diagnostic requires response body as well as HTTP success', () => {
  assert.equal(diagnosticVerdict([]).status, 'incomplete');
  assert.equal(diagnosticVerdict([{ status: 200, body: { stage: 'mounted' } }]).status, 'incomplete');
  assert.equal(diagnosticVerdict([{ status: 200, response: { ok: false }, body: { stage: 'mounted' } }]).status, 'fail');
  assert.equal(diagnosticVerdict([{ status: 200, response: { ok: true }, body: { stage: 'mounted' } }]).status, 'pass');
});

test('exact-session recovery rejects replay, foreign authority, same process and missing identity', () => {
  assert.equal(recoveryVerdict(fixture.before, fixture.after).status, 'pass');
  for (const changed of [
    { ...fixture.after, userIds: ['turn-a', 'turn-b', 'replayed-turn'] },
    { ...fixture.after, tab: { ...fixture.after.tab, agent_session_id: 'foreign' } },
    { ...fixture.after, process: fixture.before.process },
  ]) assert.equal(recoveryVerdict(fixture.before, changed).status, 'fail');
  assert.equal(recoveryVerdict(fixture.before, { ...fixture.after, process: {} }).status, 'incomplete');
  assert.deepEqual(userIds({ messages: [{ kind: 'user', id: 'a' }, { kind: 'assistant', id: 'b' }] }), ['a']);
  assert.throws(() => userIds({ messages: [{ kind: 'user' }] }));
  assert.throws(() => userIds({ messages: [{ kind: 'user', id: 'a' }, { kind: 'user', id: 'a' }] }));
  assert.deepEqual(tabAuthority({ ...fixture.before.tab, secret: 'hidden' }), fixture.before.tab);
});

test('recovery settles on helper exit and AgentReady within microtasks even when child pipes never close', async () => {
  const child = new EventEmitter();
  child.kill = () => assert.fail('No cancellation expected');
  const signal = new AbortController().signal;
  let ready;
  const recovered = new Promise(resolve => { ready = resolve; });
  const settled = settleRecovery({ exit: processExit(child, signal), recovered,
    readFinal: () => recoveryVerdict(fixture.before, fixture.after) });
  child.emit('exit', 0, null);
  ready({ phase: 'AgentReady' });
  const outcome = await Promise.race([settled, new Promise(resolve => {
    let turns = 0;
    const check = () => { if (++turns === 20) resolve('still waiting on pipe close'); else queueMicrotask(check); };
    queueMicrotask(check);
  })]);
  assert.equal(outcome.status, 'pass');
  assert.equal(child.listenerCount('exit'), 0);
});

test('exit helper failure and cancellation cannot produce a passing recovery receipt', async () => {
  const child = new EventEmitter(), control = new AbortController();
  let killed = 0; child.kill = () => { killed++; };
  const exited = processExit(child, control.signal);
  control.abort(Error('deadline'));
  await assert.rejects(exited, /deadline/);
  assert.equal(killed, 1);
  const failed = processExit(child, new AbortController().signal);
  child.emit('exit', 1, null);
  await assert.rejects(failed, /failed/);
});

test('collector deadline persists incomplete recovery independent of an uncooperative child', async () => {
  const control = new AbortController(), writes = [];
  const capture = captureBounded({ signal: control.signal, collect: () => new Promise(() => {}), persist: value => writes.push(value) });
  await Promise.resolve(); control.abort(Error('deadline'));
  const result = await capture;
  assert.equal(result.status, 'incomplete');
  assert.equal(writes.length, 1);
  assert.equal(evidenceVerdict({ schema: 1, scenario: 'exact-session-recovery', ...result, verdict: { status: 'pass' } }), 'incomplete');
});

test('receipt validation never upgrades dry-run or partial capture to a journey PASS', () => {
  const common = { schema: 1, binarySha: 'fixture-sha', listener: { pid: 1, started_us: 2 }, finalListener: { pid: 1, started_us: 2 }, status: 'completed' };
  assert.equal(evidenceVerdict({ ...common, scenario: 'exact-session-recovery', before: fixture.before, after: fixture.after }), 'pass');
  assert.equal(evidenceVerdict({ ...common, finalListener: { pid: 1, started_us: 3 }, scenario: 'exact-session-recovery', before: fixture.before, after: fixture.after }), 'incomplete');
  assert.equal(evidenceVerdict({ ...common, scenario: 'exact-session-recovery', mode: 'dry-run', before: fixture.before }), 'validated');
  assert.equal(evidenceVerdict({ ...common, scenario: 'browser-evidence', mode: 'dry-run', target: { id: 'fixture' } }), 'validated');
  assert.equal(evidenceVerdict({ ...common, scenario: 'browser-evidence', captureStatus: 'captured' }), 'incomplete');
  const rows = phaseBreakdown(fixture.phases, fixture.state);
  assert.equal(evidenceVerdict({ schema: 1, scenario: 'phase-origins', status: 'captured', rows }), 'captured');
  rows[0].origins.socket_bound.value = null;
  assert.equal(evidenceVerdict({ schema: 1, scenario: 'phase-origins', status: 'captured', rows }), 'incomplete');
});

test('endpoint and command options fail closed on foreign origins and missing values', () => {
  assert.equal(loopback('http://127.0.0.1:12345').port, '12345');
  for (const url of ['http://example.com', 'http://user:secret@127.0.0.1', 'http://127.0.0.1?token=secret']) assert.throws(() => loopback(url));
  assert.throws(() => options(['--expect-build', '--dry-run']));
  assert.throws(() => options(['--expect-build', 'sha', '--out', 'dir', '--typo']));
  assertSameListener({ pid: 1, started_us: 2 }, { pid: 1, started_us: 2 });
  assert.throws(() => assertSameListener({ pid: 1, started_us: 2 }, { pid: 1, started_us: 3 }), /changed/);
});

test('browser receipt requires every byte artifact and does not label a capture as a journey pass', () => {
  const identity = assetIdentity('http://127.0.0.1:7686/a.js', Buffer.from('abc'), 'fixture');
  const buffer = { type: 'normal', text: 'fixture', length: 1, baseY: 0, viewportY: 0 };
  const receipt = { schema: 1, scenario: 'browser-evidence', status: 'completed', binarySha: 'sha',
    listener: { pid: 1, started_us: 2 }, finalListener: { pid: 1, started_us: 2 }, index: identity, failures: [],
    assets: [{ loaded: identity, served: identity }],
    streams: [{ status: 200, artifact: { file: 'output-0.sse', bytes: 3, sha256: identity.sha256 } }],
    after: { terminals: [{ tabId: 'own', active: buffer, normal: buffer, alternate: buffer, ancestors: [{}] }] },
    diagnostics: [{ status: 200, response: { ok: true }, body: { stage: 'mounted' } }],
  };
  assert.equal(evidenceVerdict(receipt), 'captured');
  verifyByteArtifacts(receipt, '.', () => Buffer.from('abc'));
  assert.throws(() => verifyByteArtifacts(receipt, '.', () => Buffer.from('abd')), /hash/);
  assert.throws(() => verifyByteArtifacts(receipt, '.', () => { throw Error('missing file'); }), /missing/);
  receipt.streams[0].artifact.file = '../escape';
  assert.throws(() => verifyByteArtifacts(receipt, '.', () => Buffer.from('abc')), /escapes/);
  receipt.streams[0].artifact = null;
  assert.equal(evidenceVerdict(receipt), 'incomplete');
});
