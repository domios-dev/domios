import assert from 'node:assert/strict';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { admitDraft, draftNotes, qualificationTag, requireQualificationPush, prepareDraft, pruneSupersededDrafts,
  resetPlatform, sealDraft, supersededDrafts, uploadDraft } from './release-qualification-draft.mjs';
import { checksumManifest, promoteQualifiedDrafts } from './release-promote.mjs';
import { fetchPlatformEvidence, PLATFORMS, storeAsync } from './release-dry-run-manifest.mjs';

const tag = 'v1.20.1';
const sourceSha = 'a'.repeat(40);
const runId = '123';
const repository = 'domios-dev/domios';
const draft = { databaseId: 7, isDraft: true, tagName: qualificationTag(tag, runId), targetCommitish: sourceSha,
  body: draftNotes(sourceSha, runId) + '\n<!-- qualification-complete -->' };
const identity = { tag, sourceSha, runId, repository, env: {} };

test('capture can load draft helpers while its manifest module is still evaluating', () => {
  const manifest = new URL('./release-dry-run-manifest.mjs', import.meta.url).href;
  const draftModule = new URL('./release-qualification-draft.mjs', import.meta.url).href;
  const run = restoreCycle => {
    const loader = `export async function load(url, context, nextLoad) {
      const loaded = await nextLoad(url, context);
      if (url === ${JSON.stringify(draftModule)} && ${restoreCycle}) return { ...loaded,
        source: Buffer.from(loaded.source).toString('utf8').replace('./release-asset-inventory.mjs', './release-promote.mjs') };
      if (url !== ${JSON.stringify(manifest)}) return loaded;
      const source = Buffer.from(loaded.source).toString('utf8');
      const entrypoint = source.lastIndexOf('if (process.argv[1]');
      if (entrypoint < 0) throw new Error('Capture entrypoint not found');
      return { ...loaded, source: source.slice(0, entrypoint) +
        "await import('./release-qualification-draft.mjs');" };
    }`;
    return spawnSync(process.execPath, ['--input-type=module', '-e', `
      import { register } from 'node:module';
      register(${JSON.stringify('data:text/javascript,' + encodeURIComponent(loader))});
      await import(${JSON.stringify(manifest)});
    `], { encoding: 'utf8', timeout: 10_000 });
  };
  const broken = run(true);
  assert.equal(broken.status, 13, broken.stderr);
  assert.match(broken.stderr, /unsettled top-level await/i);
  const captured = run(false);
  assert.equal(captured.status, 0, captured.stderr);
});

test('qualification mutation rejects dispatches, foreign refs, actors and sources', () => {
  const env = { GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/heads/release/next', GITHUB_SHA: sourceSha,
    GITHUB_RUN_ID: runId, GITHUB_ACTOR: 'rollacode', GITHUB_TRIGGERING_ACTOR: 'rollacode' };
  assert.doesNotThrow(() => requireQualificationPush(tag, sourceSha, runId, env));
  for (const changed of [{ GITHUB_EVENT_NAME: 'workflow_dispatch' }, { GITHUB_REF: 'refs/heads/master' },
    { GITHUB_REF: 'refs/heads/release/1.20.1' },
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
  const create = calls.find(args => args[1] === 'create');
  assert.deepEqual(create.slice(0, 3), ['release', 'create', 'qualification-v1.20.1-123']);
  assert.ok(create.includes('--draft'));
  assert.ok(create.includes(sourceSha));
  assert.equal(create[create.indexOf('--title') + 1], 'Domios 1.20.1');
  assert.equal(calls.filter(args => args.includes('DELETE')).length, 0);
});

test('a version that is already published refuses a new qualification draft', async () => {
  const calls = [];
  await assert.rejects(prepareDraft({ ...identity, run: async args => {
    calls.push(args);
    if (args[1] === 'view' && args[2] === tag) return JSON.stringify({ ...draft, isDraft: false, tagName: tag });
    if (args[1] === 'view') throw new Error('release not found');
    return '';
  } }), /already published/);
  assert.equal(calls.filter(args => args[1] === 'create').length, 0);
});

test('superseded drafts cover this and older versions, including untagged placeholders, never public or foreign rows', () => {
  const marker = draftNotes(sourceSha, runId);
  const releases = [
    { id: 1, draft: true, tag_name: 'qualification-v1.20.1-122', body: marker },
    { id: 2, draft: true, tag_name: 'qualification-v1.9.9-1', body: marker },
    { id: 3, draft: true, tag_name: 'untagged-c3eb00dc220db1ab6f0c', name: 'Domios v1.20.0', body: marker },
    { id: 4, draft: true, tag_name: 'qualification-v1.20.2-130', body: marker },
    { id: 5, draft: true, tag_name: 'untagged-abc', name: 'Domios 1.21.0', body: marker },
    { id: 6, draft: false, tag_name: 'qualification-v1.20.0-1', body: marker },
    { id: 7, draft: true, tag_name: 'untagged-def', name: 'Domios v1.19.0', body: 'owner notes draft' },
    { id: 8, draft: true, tag_name: 'qualification-v1.100.0-5', body: marker },
    { id: 9, draft: true, tag_name: 'plugin-x-v1.0.0', name: 'Domios v1.0.0', body: marker },
  ];
  assert.deepEqual(supersededDrafts(releases, tag).map(row => row.id), [1, 2, 3]);
});

test('pruning requires the published release and deletes only superseded drafts', async () => {
  const marker = draftNotes(sourceSha, runId);
  const releases = [{ id: 4, draft: true, tag_name: 'qualification-v1.20.1-122', body: marker },
    { id: 5, draft: false, tag_name: 'v1.20.1', body: marker }, { id: 6, draft: true, tag_name: 'qualification-v1.21.0-200', body: marker }];
  const deleted = [];
  const run = published => async args => {
    if (args[1] === 'view') return JSON.stringify({ ...draft, isDraft: !published, tagName: tag });
    if (args.includes('DELETE')) { deleted.push(args.at(-1)); return ''; }
    return releases.map(row => JSON.stringify(row)).join('\n');
  };
  await assert.rejects(pruneSupersededDrafts({ tag, repository, env: {}, run: run(false) }), /not published/);
  assert.deepEqual(deleted, []);
  assert.deepEqual(await pruneSupersededDrafts({ tag, repository, env: {}, run: run(true) }), ['qualification-v1.20.1-122']);
  assert.deepEqual(deleted, ['repos/domios-dev/domios/releases/4']);
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
    if (args.includes('PATCH')) {
      assert.ok(args.includes(`tag_name=${draft.tagName}`));
      return '';
    }
    if (args.includes('DELETE')) return '';
    return (args.at(-1).includes('/assets?') ? assets : releases).map(row => JSON.stringify(row)).join('\n');
  } });
  assert.deepEqual(calls.filter(args => args.includes('DELETE')).map(args => args.at(-1)), [
    'repos/domios-dev/domios/releases/4', 'repos/domios-dev/domios/releases/assets/8',
    'repos/domios-dev/domios/releases/assets/9', 'repos/domios-dev/domios/releases/assets/10',
  ]);
});

test('public checksums list every platform payload from verified digests and refuse an unverified one', () => {
  const row = (name, hex) => ({ name, digest: `sha256:${hex.repeat(64)}`, state: 'uploaded' });
  const assets = [row('Domios_1.20.1_x64-setup.exe', 'b'), row('Domios_1.20.1_amd64.AppImage', 'a'),
    row('domios-headless-v1.20.1-linux-x86_64.tar.gz', 'c'), row('latest.json', 'd'), row('qualification-linux-x86_64.json', 'e')];
  assert.equal(checksumManifest(assets), [
    `${'a'.repeat(64)}  Domios_1.20.1_amd64.AppImage`, `${'b'.repeat(64)}  Domios_1.20.1_x64-setup.exe`,
    `${'c'.repeat(64)}  domios-headless-v1.20.1-linux-x86_64.tar.gz`, ''].join('\n'));
  assert.throws(() => checksumManifest([{ ...assets[0], digest: null }]), /no uploaded sha256/);
  assert.throws(() => checksumManifest([{ ...assets[0], state: 'starter' }]), /no uploaded sha256/);
  assert.throws(() => checksumManifest([assets[3]]), /no platform payloads/);
});

test('sealing preserves the unpublished tag and verifies the retained identity and seal', async () => {
  let updated;
  await sealDraft({ ...identity, run: async args => {
    if (args.includes('PATCH')) {
      assert.ok(args.includes(`tag_name=${draft.tagName}`));
      updated = { ...draft, body: args.at(-1).slice('body='.length) };
      return '';
    }
    return JSON.stringify(updated ?? draft);
  } });
  assert.ok(updated.body.includes('<!-- qualification-complete -->'));
  let reads = 0;
  await assert.rejects(sealDraft({ ...identity, run: async args => {
    if (args.includes('PATCH')) return '';
    return JSON.stringify(++reads === 1 ? draft : { ...draft, tagName: 'untagged-placeholder' });
  } }), /Foreign qualification draft/);
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

test('waived publication cannot restore a missing hidden draft', async () => {
  await assert.rejects(promoteQualifiedDrafts({ tag, sourceSha, runId, work: 'unused',
    repositories: [['domios-dev/domios', 'token']], requireDraft: true,
    drafts: { readDraft: async () => null, admitDraft, qualificationTag },
    fetchEvidence: () => assert.fail('waiver must not fetch fallback bytes'),
    run: () => assert.fail('waiver must not mutate a missing draft'),
  }), /requires complete existing qualification drafts/);
});
