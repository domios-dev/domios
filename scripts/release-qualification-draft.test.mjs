import assert from 'node:assert/strict';
import test from 'node:test';
import { admitDraft, draftNotes, qualificationTag, requireQualificationPush, prepareDraft, resetPlatform, uploadDraft } from './release-qualification-draft.mjs';
import { promoteQualifiedDrafts } from './release-promote.mjs';
import { fetchPlatformEvidence, PLATFORMS, storeAsync } from './release-dry-run-manifest.mjs';

const tag = 'v1.20.1';
const sourceSha = 'a'.repeat(40);
const runId = '123';
const repository = 'domios-dev/domios';
const draft = { databaseId: 7, isDraft: true, tagName: qualificationTag(tag, runId), targetCommitish: sourceSha,
  body: draftNotes(sourceSha, runId) + '\n<!-- qualification-complete -->' };
const identity = { tag, sourceSha, runId, repository, env: {} };

test('qualification mutation rejects dispatches, foreign actors, sources and versions', () => {
  const env = { GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/heads/release/1.20.1', GITHUB_SHA: sourceSha,
    GITHUB_RUN_ID: runId, GITHUB_ACTOR: 'rollacode', GITHUB_TRIGGERING_ACTOR: 'rollacode' };
  assert.doesNotThrow(() => requireQualificationPush(tag, sourceSha, runId, env));
  for (const changed of [{ GITHUB_EVENT_NAME: 'workflow_dispatch' }, { GITHUB_REF: 'refs/heads/master' },
    { GITHUB_SHA: 'b'.repeat(40) }, { GITHUB_RUN_ID: '124' }, { GITHUB_ACTOR: 'other' }, { GITHUB_TRIGGERING_ACTOR: 'other' }]) {
    assert.throws(() => requireQualificationPush(tag, sourceSha, runId, { ...env, ...changed }));
  }
});

test('draft admission binds source, version, run and hidden state', () => {
  assert.equal(admitDraft(draft, tag, sourceSha, runId), 7);
  for (const changed of [{ isDraft: false }, { tagName: 'v1.20.0' }, { targetCommitish: 'b'.repeat(40) },
    { body: draftNotes(sourceSha, '124') }, { body: draftNotes('b'.repeat(40), runId) }]) {
    assert.throws(() => admitDraft({ ...draft, ...changed }, tag, sourceSha, runId));
  }
});

test('same-run preparation retains draft and never recreates it on a failed-job rerun', async () => {
  const calls = [];
  await prepareDraft({ ...identity, run: async args => { calls.push(args); return JSON.stringify(draft); } });
  assert.equal(calls.length, 1);
  assert.equal(calls[0][1], 'view');
});

test('new run creates a hidden draft without a git tag or deleting failed evidence', async () => {
  const calls = [];
  await prepareDraft({ ...identity, run: async args => {
    calls.push(args);
    if (args[1] === 'view') throw new Error('release not found');
    return '';
  } });
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[1].slice(0, 3), ['release', 'create', 'qualification-v1.20.1-123']);
  assert.ok(calls[1].includes('--draft'));
  assert.ok(calls[1].includes(sourceSha));
});

test('platform reset supersedes only older same-version drafts and clears only its assets', async () => {
  const calls = [];
  const releases = [{ id: 4, draft: true, tag_name: 'qualification-v1.20.1-122' },
    { id: 5, draft: false, tag_name: 'v1.20.1' }, { id: 6, draft: true, tag_name: 'qualification-v1.20.0-121' }];
  const assets = [{ id: 8, name: 'Domios_1.20.1_amd64.AppImage' }, { id: 9, name: 'qualification-linux-x86_64.json' },
    { id: 10, name: 'updater-linux-x86_64.json' }, { id: 11, name: 'Domios_1.20.1_arm64.deb' }];
  await resetPlatform({ ...identity, platform: 'linux-x86_64', run: async args => {
    calls.push(args);
    if (args[1] === 'view') return JSON.stringify(draft);
    if (args.includes('DELETE')) return '';
    return (args.at(-1).includes('/assets?') ? assets : releases).map(row => JSON.stringify(row)).join('\n');
  } });
  assert.deepEqual(calls.filter(args => args.includes('DELETE')).map(args => args.at(-1)), [
    'repos/domios-dev/domios/releases/4', 'repos/domios-dev/domios/releases/assets/8',
    'repos/domios-dev/domios/releases/assets/9', 'repos/domios-dev/domios/releases/assets/10',
  ]);
});

test('uploads use the admitted numeric ID, check API digests, and complete only after all bytes', async () => {
  const files = [{ name: 'Domios_1.20.1_amd64.AppImage', sha256: 'c'.repeat(64), size: 42 }];
  const uploaded = [];
  const options = { ...identity, directory: 'out', receipt: { platform: 'linux-x86_64', files },
    run: async args => args[1] === 'view' ? JSON.stringify(draft) : '',
    upload: async asset => { uploaded.push(asset); return { ...files[0], digest: `sha256:${files[0].sha256}`, state: 'uploaded' }; } };
  await uploadDraft(options);
  assert.deepEqual(uploaded.map(asset => [asset.id, asset.name]), [[7, files[0].name], [7, 'qualification-linux-x86_64.json']]);
  await assert.rejects(uploadDraft({ ...options, upload: async () => ({ ...files[0], digest: 'sha256:' + 'd'.repeat(64), state: 'uploaded' }) }), /differs/);
});

test('publication reads draft metadata once and performs no retained-byte operation', async () => {
  let reads = 0;
  await promoteQualifiedDrafts({ ...identity, work: 'out', repositories: [[repository, 'test-token']],
    drafts: { readDraft: async () => { reads++; return draft; }, admitDraft, qualificationTag },
    run: async args => { assert.ok(args.includes(`repos/${repository}/releases/7`)); },
    fetchEvidence: () => assert.fail('unexpected download'), publish: () => assert.fail('unexpected upload'),
    readReceipt: () => assert.fail('unexpected receipt read') });
  assert.equal(reads, 1);
});

test('foreign draft refuses before fallback or release mutation', async () => {
  await assert.rejects(promoteQualifiedDrafts({ ...identity, work: 'out', repositories: [[repository, 'test-token']],
    drafts: { readDraft: async () => ({ ...draft, body: draftNotes(sourceSha, '124') }), admitDraft, qualificationTag },
    run: () => assert.fail('unexpected mutation'), fetchEvidence: () => assert.fail('unexpected fallback') }), /Foreign/);
});

test('missing draft fetches evidence once, restores six platforms and seals only after uploads', async () => {
  const events = [];
  let created = false;
  await promoteQualifiedDrafts({ ...identity, work: 'out', repositories: [[repository, 'test-token']],
    drafts: { readDraft: async () => created ? draft : null, admitDraft, qualificationTag,
      prepareDraft: async () => { created = true; events.push('create'); }, sealDraft: async () => events.push('seal') },
    run: async () => {}, fetchEvidence: async () => events.push('fetch'), readReceipt: async () => ({ files: [] }),
    publish: async task => { events.push(task.platform); return []; } });
  assert.deepEqual(events, ['fetch', 'create', ...PLATFORMS, 'seal']);
});

test('platform evidence downloads all start before waiting for any platform', async () => {
  const gate = Promise.withResolvers();
  const started = [];
  const work = fetchPlatformEvidence(runId, 'out', async (verb, key) => { started.push([verb, key]); await gate.promise; });
  assert.equal(started.length, PLATFORMS.length);
  gate.resolve();
  await work;
});

test('async downloads retain the one-time timeout recovery and refuse transport failures', async () => {
  let attempts = 0;
  await storeAsync('get-prefix', 'key', 'out', { log() {}, run: async () => {
    if (++attempts === 1) throw Object.assign(new Error('deadline'), { code: 'ETIMEDOUT' });
  } });
  assert.equal(attempts, 2);
  attempts = 0;
  await assert.rejects(storeAsync('get-prefix', 'key', 'out', { run: async () => { attempts++; throw new Error('denied'); } }), /denied/);
  assert.equal(attempts, 1);
});
