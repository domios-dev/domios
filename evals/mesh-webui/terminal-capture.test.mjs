import assert from 'node:assert/strict';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armTerminalEvents, captureTerminal, readTerminalState, waitTerminalEvent, releaseTerminalEvents, semanticCaptureEligible, captureEventRates } from './terminal-capture.mjs';

function fixture({ visible = true, count = 1, bufferType = 'normal' } = {}) {
  const cell = {
    getChars: () => '界', getWidth: () => 2, getFgColor: () => 0x123456,
    getBgColor: () => 0, getFgColorMode: () => 0x3000000, getBgColorMode: () => 0,
    isBold: () => 1, isItalic: () => 0, isUnderline: () => 0,
    isInverse: () => 0, isInvisible: () => 0,
  };
  const term = { cols: 1, rows: 1, options: { cursorStyle: 'bar', cursorBlink: false },
    _core: { coreService: { isCursorHidden: true } },
    buffer: { active: { type: bufferType, length: 1, baseY: 0, viewportY: 0,
      cursorX: 0, cursorY: 0,
      getLine: () => ({ length: 1, isWrapped: false, getCell: () => cell, translateToString: () => '界' }) } } };
  const host = { __codetermTerminal: term, getBoundingClientRect: () => ({ x: 2, y: 3, width: 80, height: 24 }) };
  const root = { dataset: { tabId: 'mesh:fixture/tab' }, getClientRects: () => [1],
    querySelector: selector => selector.includes('terminal-frame')
      ? { dataset: { terminalVisible: String(visible) } } : host };
  return { querySelectorAll: () => Array(count).fill(root) };
}

test('actual buffer Unicode, colors and alternate state survive serialization', () => {
  const original = globalThis.document;
  try {
    globalThis.document = fixture({ bufferType: 'alternate' });
    const state = readTerminalState({ tabId: 'mesh:fixture/tab' });
    assert.equal(state.buffer, 'alternate');
    assert.equal(state.lines[0].cells[0].chars, '界');
    assert.equal(state.lines[0].cells[0].fg, 0x123456);
    assert.equal(state.cursor.hidden, true);
  } finally { globalThis.document = original; }
});

test('hidden chat terminal and duplicate tab roots cannot supply renderer proof', () => {
  const original = globalThis.document;
  try {
    globalThis.document = fixture({ visible: false });
    assert.throws(() => readTerminalState({ tabId: 'mesh:fixture/tab' }), /hidden/);
    globalThis.document = fixture({ count: 2 });
    assert.throws(() => readTerminalState({ tabId: 'mesh:fixture/tab' }), /found 2/);
  } finally { globalThis.document = original; }
});

function capturePage(revisions, { failWait = false, truncated = false, texts } = {}) {
  let index = 0;
  let shots = 0;
  let reads = 0;
  const waits = [];
  const state = { truncated, viewportY: 0, rows: 1, lines: [{ y: 0, text: 'fixed' }] };
  return { waits, get shots() { return shots; },
    evaluate: async (fn, args) => {
      if (fn === readTerminalState) {
        const result = structuredClone(state);
        if (texts) result.lines[0].text = texts[reads++];
        return result;
      }
      if (fn.name === 'readProbeRevision') return revisions[index++];
      if (fn.name === 'readProbeEvents') return [{ type: 'render', revision: revisions[index - 1] }];
      waits.push(args);
      if (failWait) throw new Error('event failure deadline');
      return { type: 'write-parsed', revision: args.after + 1 };
    },
    screenshot: async ({ path }) => { shots++; await writeFile(path, 'fake image'); },
  };
}

async function withCaptureDirectory(run) {
  const temp = await mkdtemp(join(tmpdir(), 'b3-capture-'));
  try { await run(join(temp, 'capture')); }
  finally { await rm(temp, { recursive: true, force: true }); }
}

test('revision changes with identical text require an event before a stable second attempt', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 0, 0, 1, 2, 2, 2, 2]);
    const result = await captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: {} });
    assert.equal(result.revision, 2);
    assert.equal(page.shots, 2);
    assert.equal(page.waits.length, 1);
    assert.deepEqual(page.waits[0].types, ['write-parsed', 'render']);
    assert.equal(page.waits[0].after, 0);
    assert.ok(page.waits[0].deadlineMs > 0 && page.waits[0].deadlineMs <= 30000);
    const saved = JSON.parse(await readFile(join(directory, 'state.json'), 'utf8'));
    assert.equal(saved.attempts.length, 2);
    assert.equal(saved.attempts[0].unchanged, false);
    assert.equal(saved.attempts[1].unchanged, true);
    assert.equal(await readFile(join(directory, 'screen-attempt-1.png'), 'utf8'), 'fake image');
  });
});

test('three unstable attempts preserve evidence and fail without a fourth screenshot', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 0, 0, 1, 2, 2, 2, 3, 4, 4, 4, 5]);
    await assert.rejects(captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: {} }), /unstable surface/);
    assert.equal(page.shots, 3);
    assert.equal(page.waits.length, 2);
    const saved = JSON.parse(await readFile(join(directory, 'state.json'), 'utf8'));
    assert.equal(saved.unchanged, false);
    assert.equal(saved.attempts.length, 3);
    await assert.rejects(readFile(join(directory, 'screen.png')), { code: 'ENOENT' });
  });
});

test('a changing pre-screenshot revision skips screenshot until an event advances the probe', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 1, 1, 1, 2, 2, 2, 2]);
    await captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: {} });
    assert.equal(page.shots, 1);
    const first = JSON.parse(await readFile(join(directory, 'attempt-1.json'), 'utf8'));
    assert.equal(first.screenshot, null);
  });
});

test('event deadline failure remains unstable and preserves the attempt', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 0, 0, 1], { failWait: true });
    await assert.rejects(captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: {} }), /unstable surface/);
    assert.equal(page.shots, 1);
    const saved = JSON.parse(await readFile(join(directory, 'state.json'), 'utf8'));
    assert.equal(saved.attempts.length, 1);
    assert.equal(saved.attempts[0].waitError, 'event failure deadline');
  });
});

test('probe arming works on an HTTP browser without crypto.randomUUID', async () => {
  const window = {};
  const listener = () => ({ dispose() {} });
  const term = { onRender: listener, onResize: listener, onScroll: listener,
    onCursorMove: listener, onWriteParsed: listener, buffer: { onBufferChange: listener } };
  const root = { dataset: { tabId: 'fixture' }, getClientRects: () => [1],
    querySelector: () => ({ __codetermTerminal: term }) };
  const page = { evaluate: async (fn, args) => runInNewContext(`(${fn.toString()})(args)`, {
    args, window, crypto: {}, document: { querySelectorAll: () => [root] },
  }) };
  const id = await armTerminalEvents(page, 'fixture');
  assert.match(id, /^[0-9a-f-]{36}$/);
  assert.equal(window.__b3TerminalProbes.get(id).tabId, 'fixture');
  assert.equal(window.__b3TerminalProbes.get(id).handles.length, 6);
});


test('changed terminal text cannot pass with an unchanged probe revision', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 0, 0, 0], { failWait: true, texts: ['before', 'after'] });
    await assert.rejects(captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: {} }), /unstable surface/);
    assert.equal(page.shots, 0);
    const saved = JSON.parse(await readFile(join(directory, 'state.json'), 'utf8'));
    assert.equal(saved.before.lines[0].text, 'before');
    assert.equal(saved.after.lines[0].text, 'after');
  });
});

test('truncated state cannot become a successful stable capture', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 0, 0, 0], { truncated: true });
    await assert.rejects(captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: {} }), /incomplete capture/);
    assert.equal(page.waits.length, 0);
    const saved = JSON.parse(await readFile(join(directory, 'state.json'), 'utf8'));
    assert.equal(saved.unchanged, false);
    await assert.rejects(readFile(join(directory, 'screen.png')), { code: 'ENOENT' });
  });
});


test('transition completion already observed during reads satisfies the event wait', async () => {
  const probe = { tabId: 'fixture', revision: 7,
    events: [{ type: 'write-parsed', revision: 4 }, { type: 'render', revision: 7 }],
    waiters: new Set() };
  const window = { __b3TerminalProbes: new Map([['probe', probe]]) };
  const page = { evaluate: (fn, args) => runInNewContext(`(${fn.toString()})(args)`, {
    args, window, AbortSignal,
  }) };
  const event = await waitTerminalEvent(page, 'probe', {
    types: ['write-parsed', 'render'], after: 0,
  });
  assert.equal(event.revision, 4);
  assert.equal(probe.waiters.size, 0);
});

test('probe release cancels a pending event wait and disposes observers', async () => {
  let disposed = 0;
  const probe = { tabId: 'fixture', events: [], waiters: new Set(),
    handles: [{ dispose() { disposed++; } }] };
  const window = { __b3TerminalProbes: new Map([['probe', probe]]) };
  const page = { evaluate: (fn, args) => runInNewContext(`(${fn.toString()})(args)`, {
    args, window, AbortSignal,
  }) };
  const pending = waitTerminalEvent(page, 'probe', { type: 'render', after: 0 });
  const rejected = assert.rejects(pending, /Terminal wait cancelled/);
  assert.equal(probe.waiters.size, 1);
  await releaseTerminalEvents(page, 'probe');
  await rejected;
  assert.equal(probe.waiters.size, 0);
  assert.equal(window.__b3TerminalProbes.has('probe'), false);
  assert.equal(disposed, 1);
});

function zeroDiffStrictEvidence(tabId = 'fixture') {
  const state = { tabId, truncated: false, viewportY: 0, rows: 1,
    lines: [{ y: 0, text: 'fixed' }] };
  return { failure: 'FAIL unstable surface: three capture attempts exhausted', after: structuredClone(state),
    attempts: [1, 2, 3].map(number => ({ number, before: structuredClone(state),
      candidate: structuredClone(state), after: structuredClone(state), unchanged: false })) };
}

test('semantic mode permits fake no-op revisions while recording their delta', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 1, 2, 3]);
    await captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory, provenance: {},
      stabilityMode: 'semantic', strictExhaustionEvidence: zeroDiffStrictEvidence() });
    const saved = JSON.parse(await readFile(join(directory, 'state.json'), 'utf8'));
    assert.equal(saved.stabilityMode, 'semantic');
    assert.equal(saved.unchanged, true);
    assert.equal(saved.revisionDelta, 3);
    assert.equal(page.shots, 1);
    assert.equal(page.waits.length, 0);
  });
});

test('semantic mode still rejects changed cells around the screenshot', async () => {
  await withCaptureDirectory(async directory => {
    const page = capturePage([0, 1, 2, 3], { failWait: true, texts: ['fixed', 'fixed', 'changed'] });
    await assert.rejects(captureTerminal(page, { tabId: 'fixture', probeId: 'probe', directory,
      provenance: {}, stabilityMode: 'semantic', strictExhaustionEvidence: zeroDiffStrictEvidence() }),
    /unstable surface/);
    assert.equal(page.shots, 1);
  });
});

test('semantic eligibility requires all three zero-diff attempts from the same tab', () => {
  const evidence = zeroDiffStrictEvidence();
  assert.equal(semanticCaptureEligible(evidence, 'fixture'), true);
  assert.equal(semanticCaptureEligible(evidence, 'other'), false);
  const changed = structuredClone(evidence);
  changed.attempts[1].after.lines[0].text = 'changed';
  assert.equal(semanticCaptureEligible(changed, 'fixture'), false);
  const short = structuredClone(evidence);
  short.attempts.pop();
  assert.equal(semanticCaptureEligible(short, 'fixture'), false);
});

test('event evidence records measured render and write rates', () => {
  const rates = captureEventRates([{ type: 'render', at: 1000 },
    { type: 'write-parsed', at: 1500 }, { type: 'render', at: 2000 }]);
  assert.equal(rates.spanSeconds, 1);
  assert.equal(rates.perSecond.render, 2);
  assert.equal(rates.perSecond['write-parsed'], 1);
});
