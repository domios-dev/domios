import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { matrix, applyVerdict, key, markdown, requirements } from './matrix.mjs';
import { jsonLines, proveRecord } from './records.mjs';
import { evidencePath, writeEvidence, requireEvidence } from './evidence.mjs';
import { validateTarget, sampleCommands, waitForEvent, osSamples } from './sampling.mjs';
test('matrix covers both directions, three views, each required provider and check', () => {
  const rows = matrix();
  assert.equal(rows.length, 84);
  assert.equal(new Set(rows.map(key)).size, 84);
  assert.ok(rows.every(row => row.status === 'NOT_RUN'));
  assert.equal(rows.filter(row => row.provider === 'grok' && row.view === 'web').length, 6);
  assert.match(markdown(rows), /NOT_RUN/);
});
test('missing observation cannot become PASS and a FAIL cannot be overwritten', () => {
  const rows = matrix();
  const row = { ...rows[0], status: 'PASS', evidence: ['view.json'] };
  assert.throws(() => applyVerdict(rows, row), /Missing observation/);
  const observations = Object.fromEntries(requirements[1].map(name => [name, true]));
  applyVerdict(rows, { ...row, observations });
  applyVerdict(rows, { ...row, status: 'FAIL', repro: { steps: ['observe stale Done'], tabId: 'a', sessionId: 's', logLines: [3] } });
  assert.throws(() => applyVerdict(rows, { ...row, observations }), /Preserve FAIL/);
});
test('record proof refuses another session and malformed records', () => {
  const records = jsonLines('{"session":"own","model":"cheap"}\n{"session":"foreign","model":"expensive"}\n');
  const claim = { sessionId: 'own', tabId: 'tab', sourcePath: 'raw.jsonl', sessionPath: ['session'], valuePath: ['model'], expected: 'cheap' };
  assert.deepEqual(proveRecord(records, claim).lines, [1]);
  assert.throws(() => proveRecord(records, { ...claim, expected: 'expensive' }), /No claim/);
  assert.throws(() => jsonLines('{}\npartial'), /line 2/);
  assert.throws(() => proveRecord(records, { ...claim, sessionId: '' }), /Bound session/);
});
test('artifact paths are confined, immutable and nonempty', async () => {
  const root = await mkdtemp(join(tmpdir(), 'mesh-evidence-'));
  try {
    assert.throws(() => evidencePath(root, '../foreign'), /stay/);
    const artifact = await writeEvidence(root, 'failure.json', '{"status":"FAIL"}');
    assert.equal(artifact.sha256.length, 64);
    await assert.rejects(writeEvidence(root, 'failure.json', 'PASS'), { code: 'EEXIST' });
    assert.equal((await requireEvidence(root, ['failure.json']))[0].sha256, artifact.sha256);
    assert.match(await readFile(join(root, 'failure.json'), 'utf8'), /FAIL/);
    await assert.rejects(requireEvidence(root, ['missing']), { code: 'ENOENT' });
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('sampling is DEV constrained and mesh ids qualify all selected-tab reads', () => {
  validateTarget('aiderqa', 7697);
  validateTarget('meshqa', 7698);
  for (const port of [7685, 7686]) assert.throws(() => validateTarget('aiderqa', port));
  assert.ok(sampleCommands('tab', 'session', 'abcdef').every(args => args.includes('--mesh')));
  assert.throws(() => sampleCommands('bad;id', 'session', 'abcdef'));
  assert.ok(osSamples.macos.length && osSamples.windows.length);
});
test('readiness is an observed event; stream end is a failure', async () => {
  async function* events(signal) { assert.equal(signal.aborted, false); yield { ready: false }; yield { ready: true }; }
  assert.deepEqual(await waitForEvent(events, event => event.ready), { ready: true });
  await assert.rejects(waitForEvent(events, () => false), /ended/);
});
