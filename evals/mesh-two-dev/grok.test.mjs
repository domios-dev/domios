import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { jsonLines } from './records.mjs';
import { summarizeRecords, proveMediumChat } from './provider-records.mjs';
import { verifyGrokPath } from './grok-records.mjs';
const fixture = new URL('./fixtures/grok-updates.jsonl', import.meta.url);
const binding = { provider: 'grok', tabId: 'fixture-tab', sessionId: 'fixture-session', cwd: '/work/qa space', originalPath: '/fixture/.grok/sessions/%2Fwork%2Fqa%20space/fixture-session/updates.jsonl' };
test('Grok fixture proves model from the next own turn and counts fragmented prompts once', async () => {
  const rows = jsonLines(await readFile(fixture, 'utf8'));
  const summary = summarizeRecords(rows, binding);
  assert.deepEqual(summary.turns.map(turn => turn.id), ['fixture-session:prompt:0', 'fixture-session:prompt:1']);
  assert.deepEqual(summary.models, [{ model: 'grok-4.6', line: 2 }, { model: 'grok-4.5', line: 7 }]);
  const medium = structuredClone(rows);
  for (let index = 2; index < 15; index++) {
    const row = structuredClone(rows.at(-1));
    row.params.update._meta.promptIndex = index;
    medium.push(row);
  }
  assert.deepEqual(proveMediumChat(summarizeRecords(medium, binding), ['grok-4.6', 'grok-4.5']).switchLines, [2, 7]);
});
test('Grok fixture refuses foreign sessions, model conflicts and missing turn identities', async () => {
  const rows = jsonLines(await readFile(fixture, 'utf8'));
  for (const mutate of [
    value => value[0].params.sessionId = 'sibling',
    value => value[2].params.update._meta.modelId = 'foreign-model',
    value => delete value[1].params.update._meta.promptIndex,
    value => delete value[1].params.update._meta.modelId,
  ]) { const value = structuredClone(rows); mutate(value); assert.throws(() => summarizeRecords(value, binding)); }
});
test('Grok binds the encoded absolute cwd and exact session directory on both OS paths', () => {
  verifyGrokPath(binding.originalPath, binding.cwd, binding.sessionId);
  verifyGrokPath('C:\\Users\\qa\\.grok\\sessions\\C%3A%5Cwork%5Cqa\\fixture-session\\updates.jsonl', 'C:\\work\\qa', 'fixture-session');
  for (const value of [
    { ...binding, originalPath: binding.originalPath.replace('fixture-session/', 'newest-session/') },
    { ...binding, cwd: '/work/other' },
    { ...binding, originalPath: binding.originalPath.replace('updates.jsonl', 'other.jsonl') },
  ]) assert.throws(() => verifyGrokPath(value.originalPath, value.cwd, value.sessionId));
});
