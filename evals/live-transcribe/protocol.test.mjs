import assert from 'node:assert/strict';
import test from 'node:test';
import { createLookup, createSession, MIN_PAGE_CHARS, project, toolSchemas } from './protocol.mjs';
import { benchmark, evaluateScenario, httpModel, loadScenario, markdown, score } from './runner.mjs';

const tabs = [{ id: 'qa', title: 'QA' }];
const ack = through_seq => ({ name: 'ack', args: { through_seq, note: 'Processed' } });
const hold = through_seq => ({ name: 'mark_seen', args: { through_seq } });
const route = {
  name: 'route', args: { target_tab: 'qa', payload: 'Validate the export dialog.', rationale: 'Requested' },
};

test('Format v1 frames separate context, unread, seen and live text', () => {
  const rows = ['first', 'second', 'third', 'fourth', 'unfinished', 'continue 🦉\nplease']
    .map((text, index) => ({ seq: index + 1, text }));
  rows[4].attachments = [{ id: 'shot', seq: 99 }];
  const page = project(rows, { acked_seq: 4, seen_seq: 5 }, { liveTail: 'still \\ dictating' });
  assert.equal(page.text, [
    'AGENT TRANSCRIPT v1', '[CONTEXT]', '2 | second', '3 | third', '4 | fourth', '',
    '[UNREAD]', '6 | continue 🦉\\nplease', '', '[SEEN]', '5 | unfinished', '',
    '[LIVE]', 'still \\\\ dictating',
  ].join('\n'));
  assert.deepEqual(page.meta, { acked_seq: 4, seen_seq: 5, through_seq: 6, committed_seq: 6, next: null });
  assert.deepEqual(page.attachments, [{ id: 'shot', seq: 5 }]);
});

test('pagination counts Unicode scalars and advances through fully exposed rows', () => {
  const cursor = { acked_seq: 0, seen_seq: 1 };
  const rows = [
    { seq: 1, kind: 'paused', text: '🦉', attachments: [{ id: 'shot' }] },
    { seq: 2, kind: 'resumed' },
  ];
  const seen = [], unread = [], refs = [];
  let position;
  let completed = 0;
  do {
    const page = project(rows, cursor, { position, maxChars: MIN_PAGE_CHARS, liveTail: 'x' });
    assert.equal(Array.from(page.text).length, MIN_PAGE_CHARS);
    unread.push(page.text.split('\n[UNREAD]\n')[1].split('\n[SEEN]\n')[0]);
    seen.push(page.text.split('\n[SEEN]\n')[1].split('\n[LIVE]\n')[0]);
    if (page.attachments.length) assert.equal(page.meta.through_seq, 1);
    assert.ok(page.meta.through_seq >= completed);
    completed = page.meta.through_seq;
    refs.push(...page.attachments);
    position = page.meta.next;
  } while (position);
  assert.equal(seen.join(''), '1 | PAUSED 🦉\n');
  assert.equal(unread.join(''), '2 | RESUMED\n');
  assert.deepEqual(refs, [{ id: 'shot', seq: 1 }]);
  assert.equal(completed, 2);
  assert.throws(() => project(rows, cursor, { maxChars: MIN_PAGE_CHARS - 1 }), RangeError);
});

test('epoch rotation clears old notes while transferring held speech and document state', () => {
  const session = createSession(tabs);
  for (let seq = 1; seq <= 9; seq++) {
    session.append([{ text: `entry ${seq}` }]);
    const patch = { name: 'shape_doc', args: { patch: { heading: 'Log', prose: 'Started.', bullets: ['Check the export'] } } };
    const result = session.finish(seq === 1 ? [route, patch, ack(seq)] : [ack(seq)], seq);
    assert.equal(result.rotated, false);
    assert.equal(result.error, null);
  }
  session.append([{ text: 'Tell QA to...' }]);
  assert.deepEqual(session.finish([hold(10)], 10), { epoch: 1, rotated: true, error: null, effects: [] });
  const restored = createSession(tabs, JSON.parse(JSON.stringify(session.snapshot)));
  assert.equal(restored.snapshot.epoch, 2);
  assert.equal(restored.snapshot.epochTurns, 0);
  assert.deepEqual(restored.snapshot.cursor, { acked_seq: 9, seen_seq: 10 });
  assert.match(restored.pages()[0].text, /\[SEEN\]\n10 \| Tell QA/u);
  assert.deepEqual(restored.handoff().epoch_notes, []);
  assert.deepEqual(restored.handoff().document, [{ heading: 'Log', paragraphs: ['Started.'], bullets: ['Check the export'] }]);
  assert.deepEqual(restored.handoff().routes[0].args, route.args);
});

test('closed actions fail atomically and replayed committed routes produce no second effect', () => {
  const session = createSession(tabs);
  session.append([{ text: 'Please tell QA to validate the export dialog' }]);
  for (const actions of [
    [route, hold(1)], [route, ack(2)], [{ ...route, args: { ...route.args, target_tab: 'outside' } }, ack(1)],
    [route, { name: 'open_tab', args: {} }, ack(1)],
    [{ ...route, args: { ...route.args, extra: 'refuse unexpected input' } }, ack(1)],
  ]) {
    assert.ok(session.finish(actions, 1).error);
    assert.deepEqual(session.snapshot.cursor, { acked_seq: 0, seen_seq: 0 });
    assert.equal(session.snapshot.effects.length, 0);
  }
  const repeated = { ...route, args: { ...route.args, rationale: 'Different rationale' } };
  assert.equal(session.finish([route, repeated, ack(1)], 1).effects.length, 1);
  const persisted = JSON.parse(JSON.stringify(session.snapshot));
  const restored = createSession(tabs, persisted);
  assert.ok(restored.finish([route, ack(1)], 1).error);
  assert.deepEqual(restored.snapshot, persisted);
  restored.append([{ text: 'Please tell QA the same thing again' }]);
  assert.equal(restored.finish([route, ack(2)], 2).effects.length, 1);
});

test('lookup does not exceed two calls or 1,500 UTF-8 result bytes', () => {
  const corpus = [{ keywords: 'prefix', fact: 'abc' }, { keywords: 'owl', fact: '🦉'.repeat(700) }];
  const off = createLookup(corpus, false);
  assert.throws(() => off.read('owl'), /disabled/u);
  assert.ok(!toolSchemas(tabs, false).some(tool => tool.function.name === 'mem_lookup'));
  const on = createLookup(corpus, true);
  assert.equal(on.read('prefix'), 'abc');
  assert.equal(Buffer.byteLength(on.read('owl')), 1496);
  assert.throws(() => on.read('owl'), /call budget/u);
  assert.deepEqual(on.usage, { calls: 2, bytes: 1499 });
  assert.equal(Buffer.byteLength(createLookup([{ keywords: 'a', fact: 'a'.repeat(2000) }], true).read('a')), 1500);
});

test('mock benchmarks measure intent, handoff, document completeness and memory A/B', async () => {
  const report = await benchmark({ mock: true, memLookup: 'ab', maxChars: MIN_PAGE_CHARS + 10 });
  const [off, on] = report.configurations;
  assert.equal(on.errors, 0);
  assert.deepEqual(on.routing, { matched: 17, expected: 17, actual: 17, precision: 1, recall: 1 });
  assert.deepEqual(on.epochs, { hits: 3, total: 3, continuity: 1 });
  assert.deepEqual(on.restarts, { hits: 2, total: 2, continuity: 1 });
  assert.equal(on.holds.accuracy, 1);
  assert.equal(on.shapes.completeness, 1);
  assert.equal(on.ambient.falsePositiveRate, 0);
  assert.equal(off.memory.accuracy, 0);
  assert.equal(on.memory.accuracy, 1);
  assert.equal(report.delta.memoryAccuracy, 1);
  assert.deepEqual(report.delta.memoryLatencyMs, { p50: 5, p95: 5 });
  assert.deepEqual(on.latencyMs, { p50: 5, p95: 10 });
  assert.equal(on.tokensPerTurn.measured, on.turns);
  assert.ok(on.tokensPerTurn.prompt > off.tokensPerTurn.prompt);
  assert.match(markdown(report), /memory probe accuracy 100\.0 pp/u);

  const fixture = await loadScenario('long-session');
  const replay = await evaluateScenario(fixture, { mock: true });
  const resumed = replay.results.find(result => result.turn.resumeSeen);
  resumed.effects[0].args.payload = 'Add eight pixels of padding to each side of the confirmation card. See shot-export.';
  assert.equal(score(replay.results).epochs.hits, 2);
  assert.equal(score(replay.results).routing.matched, score(replay.results).routing.expected - 1);
});

test('the HTTP adapter sends the closed schema and model ID to chat completions', async () => {
  const request = httpModel({
    baseUrl: 'https://example.test/custom/v1/', model: 'local-fixture', apiKey: 'synthetic',
    async fetcher(url, options) {
      assert.equal(url.href, 'https://example.test/custom/v1/chat/completions');
      assert.equal(options.method, 'POST');
      assert.equal(options.redirect, 'error');
      assert.equal(options.headers.Authorization, 'Bearer synthetic');
      const body = JSON.parse(options.body);
      assert.equal(body.model, 'local-fixture');
      assert.deepEqual(body.tools.map(tool => tool.function.name), ['route', 'shape_doc', 'save_mem', 'ack', 'mark_seen']);
      assert.deepEqual(body.tools[0].function.parameters.properties.target_tab.enum, ['qa']);
      return { ok: true, json: async () => ({
        choices: [{ message: { role: 'assistant', content: null, tool_calls: [
          { id: 'ack-1', type: 'function', function: { name: 'ack', arguments: JSON.stringify(ack(1).args) } },
        ] } }],
      }) };
    },
  });
  const { results } = await evaluateScenario({ tabs, corpus: [], turns: [{ rows: [{ text: 'Looks like rain.' }] }] }, { request });
  assert.equal(results[0].cursorAction, 'ack', results[0].error);
  assert.equal(score(results).tokensPerTurn.prompt, null);
});
