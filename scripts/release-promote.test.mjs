import assert from 'node:assert/strict';
import test from 'node:test';
import { promotionPlan, publishConcurrently, verifyAssetDigests } from './release-promote.mjs';

const files = [
  { name: 'Domios.AppImage', sha256: 'a'.repeat(64), size: 42 },
  { name: 'updater-linux-x86_64.json', sha256: 'b'.repeat(64), size: 12 },
  { name: 'domios-headless.tar.gz', sha256: 'c'.repeat(64), size: 84 },
  { name: 'domios-headless.tar.gz.sha256', sha256: 'd'.repeat(64), size: 90 },
  { name: 'domios-headless.json', sha256: 'e'.repeat(64), size: 20 },
];
const asset = file => ({ name: file.name, digest: `sha256:${file.sha256}`, size: file.size, state: 'uploaded' });

test('promotion includes every retained asset and headless metadata; only source gets fragments', () => {
  const plan = promotionPlan({ files });
  assert.deepEqual(plan.source, files.map(file => file.name));
  assert.deepEqual(plan.mirror, files.filter(file => !file.name.startsWith('updater-')).map(file => file.name));
});

test('API digests match all receipt files regardless of API order or unrelated assets', () => {
  assert.doesNotThrow(() => verifyAssetDigests(files, [...files.map(asset).reverse(), { name: 'other' }], 'owner/repo'));
});

test('missing, malformed, or non-sha256 digests refuse publication', () => {
  for (const digest of [undefined, null, '', 'sha256:', 'md5:' + 'a'.repeat(64), 'sha256:' + 'z'.repeat(64)]) {
    assert.throws(() => verifyAssetDigests([files[0]], [{ ...asset(files[0]), digest }], 'owner/repo'), /no valid sha256 API digest/);
  }
});

test('missing and duplicate assets refuse publication', () => {
  assert.throws(() => verifyAssetDigests(files, files.slice(1).map(asset), 'owner/repo'), /expected one uploaded asset/);
  assert.throws(() => verifyAssetDigests([files[0]], [asset(files[0]), asset(files[0])], 'owner/repo'), /expected one uploaded asset/);
});

test('changed digests, sizes, states, or invalid receipt digests refuse publication', () => {
  for (const changed of [
    { digest: 'sha256:' + 'f'.repeat(64) }, { size: 43 }, { state: 'starter' },
  ]) {
    assert.throws(() => verifyAssetDigests([files[0]], [{ ...asset(files[0]), ...changed }], 'owner/repo'), /differs from its retained receipt/);
  }
  assert.throws(() => verifyAssetDigests([{ ...files[0], sha256: null }], [asset(files[0])], 'owner/repo'), /differs from its retained receipt/);
});

test('publication starts six concurrent tasks and starts another only when a slot is free', async () => {
  const started = [];
  const releases = [];
  const seventhStarted = Promise.withResolvers();
  const work = publishConcurrently([0, 1, 2, 3, 4, 5, 6], async task => {
    started.push(task);
    const gate = Promise.withResolvers();
    releases[task] = gate.resolve;
    if (task === 6) seventhStarted.resolve();
    await gate.promise;
  });
  assert.deepEqual(started, [0, 1, 2, 3, 4, 5]);
  releases[1]();
  await seventhStarted.promise;
  assert.deepEqual(started, [0, 1, 2, 3, 4, 5, 6]);
  for (const release of releases) release();
  await work;
});

test('failure stops new tasks and drains active publications before refusing', async () => {
  const gate = Promise.withResolvers();
  const started = [];
  let drained = false;
  const failure = new Error('upload refused');
  const work = publishConcurrently([0, 1, 2], async task => {
    started.push(task);
    if (task === 0) throw failure;
    await gate.promise;
    drained = true;
  }, 2);
  assert.deepEqual(started, [0, 1]);
  gate.resolve();
  await assert.rejects(work, error => error === failure);
  assert.equal(drained, true);
  assert.deepEqual(started, [0, 1]);
});

test('invalid concurrency refuses without starting work', async () => {
  for (const limit of [0, -1, 1.5, Infinity]) {
    await assert.rejects(publishConcurrently([1], () => assert.fail('unexpected publication'), limit), /invalid publication concurrency/);
  }
});

import { QUALIFICATION_JOBS, verifyQualification, selectQualificationArtifact, selectReleasePushRun,
  requireQualification } from './ci-qualification.mjs';
import { isReleasePushRun, checkQualifiedRun, REQUIRED_JOBS } from './release-promote.mjs';
import { requireQualificationWithGithub, requireReleasePrQualification } from './ci-workflow-api.mjs';
import { ciFailures } from './ci-result.mjs';
import { classifyPluginDiff, directPluginId } from './plugin-release-scope.mjs';

const repository = 'domios-dev/domios';
const sha = 'a'.repeat(40);
const branch = 'release/next';
const run = { id: 123, event: 'push', path: '.github/workflows/tests.yml', head_sha: sha, head_branch: branch,
  repository: { full_name: repository }, head_repository: { full_name: repository },
  actor: { login: 'rollacode' }, triggering_actor: { login: 'rollacode' },
  status: 'completed', conclusion: 'success', run_attempt: 1, workflow_id: 42,
  run_started_at: '2026-11-15T00:00:00Z' };
const qualifiedJobs = QUALIFICATION_JOBS.map((name, index) => ({ id: index + 1, name, run_id: run.id,
  run_attempt: 1, head_sha: sha, status: 'completed', conclusion: 'success', runner_id: 1,
  runner_name: 'builder', labels: ['hosted'], started_at: '2026-11-15T00:00:01Z',
  completed_at: '2026-11-15T00:00:03Z', steps: [{ name: 'Execute', number: 1,
    status: 'completed', conclusion: 'success', started_at: '2026-11-15T00:00:01Z', completed_at: '2026-11-15T00:00:03Z' }] }));
const receipt = { version: 2, sha, run_id: run.id, run_attempt: 1, repository, workflow: run.path,
  results: { fast: 'success', rust: 'success', warnings: 'success' },
  rust: [['full', 'Linux'], ['embeddings', 'macOS'], ['platform', 'macOS'], ['platform', 'Windows']]
    .map(([mode, os]) => ({ mode, os, sha, run_id: run.id, run_attempt: 1, repository, runner: 'builder',
      producer: mode === 'full' ? 'rust-tests' : mode === 'platform' ? 'rust-platform' : 'rust-embeddings',
      executed: 1, exit: 0, seconds: 1 })) };
const verify = (r = receipt, workflowRun = run, jobs = qualifiedJobs, attempts = [workflowRun]) =>
  verifyQualification(r, workflowRun, jobs, sha, repository, attempts);

test('only same-repository owner release push runs admit release promotion', () => {
  assert.equal(isReleasePushRun(run, sha, repository), true);
  for (const changed of [
    { event: 'pull_request' }, { event: 'pull_request_target' }, { event: 'workflow_dispatch' },
    { head_branch: 'master' }, { head_branch: 'release/1.20.0/foreign' }, { head_branch: 'release/no-version' },
    { head_sha: 'b'.repeat(40) }, { head_repository: { full_name: 'fork/domios' } },
    { repository: { full_name: 'foreign/domios' } }, { actor: { login: 'other' } },
    { triggering_actor: { login: 'other' } }, { path: '.github/workflows/other.yml' },
  ]) assert.equal(isReleasePushRun({ ...run, ...changed }, sha, repository), false, JSON.stringify(changed));
});

test('selection fails closed on absent, failed, cancelled, pending or newer failed push runs', () => {
  assert.equal(selectReleasePushRun([run], sha, branch, repository), run);
  assert.throws(() => selectReleasePushRun([], sha, branch, repository), /No completed green/);
  for (const changed of [{ conclusion: 'failure' }, { conclusion: 'cancelled' }, { status: 'in_progress' },
    { head_sha: 'b'.repeat(40) }, { head_branch: 'release/1.19.0' }, { event: 'pull_request' }])
    assert.throws(() => selectReleasePushRun([{ ...run, ...changed }], sha, branch, repository));
  assert.throws(() => selectReleasePushRun([run, { ...run, id: 124, conclusion: 'failure' }], sha, branch, repository));
});

test('qualification preserves execution receipts, warnings, attempt and SHA guards on push', () => {
  assert.equal(verify().verified_run_attempt, 1);
  for (const changed of [{ event: 'pull_request' }, { event: 'workflow_dispatch' },
    { head_repository: { full_name: 'fork/domios' } }, { head_branch: 'master' }, { head_branch: 'develop' },
    { head_branch: 'release/1.20.0/foreign' }, { actor: { login: 'other' } },
    { triggering_actor: { login: 'other' } }, { head_sha: 'b'.repeat(40) }, { conclusion: 'failure' }])
    assert.throws(() => verify(receipt, { ...run, ...changed }));
  for (const changed of [{ run_id: 999 }, { run_attempt: 2 }, { sha: 'b'.repeat(40) },
    { results: { ...receipt.results, warnings: 'failure' } },
    { rust: receipt.rust.map(row => ({ ...row, executed: 0 })) },
    { rust: receipt.rust.map(row => ({ ...row, run_attempt: 2 })) }])
    assert.throws(() => verify({ ...receipt, ...changed }));
  for (const changed of [{ run_id: 999 }, { run_attempt: 2 }, { head_sha: 'b'.repeat(40) }, { conclusion: 'failure' }])
    assert.throws(() => verify(receipt, run, qualifiedJobs.map((job, i) => i === 0 ? { ...job, ...changed } : job)));
  assert.throws(() => verify(receipt, { ...run, run_attempt: 2 }, qualifiedJobs, [run]), /attempt history/);
});

const artifact = { id: 1, name: `qualification-${sha}`, expired: false, expires_at: '2099-01-01T00:00:00Z',
  workflow_run: { id: run.id, head_sha: sha } };

test('receipt selection binds expiry, SHA and producing run rather than newest repository artifact', () => {
  const foreign = { ...artifact, id: 2, workflow_run: { id: 999, head_sha: sha } };
  assert.equal(selectQualificationArtifact([artifact, foreign], sha, Date.now(), run.id), artifact);
  for (const changed of [{ expired: true }, { expires_at: '2000-01-01T00:00:00Z' },
    { workflow_run: { id: 999, head_sha: sha } }, { workflow_run: { id: run.id, head_sha: 'b'.repeat(40) } }])
    assert.throws(() => selectQualificationArtifact([{ ...artifact, ...changed }], sha, Date.now(), run.id));
});

test('CLI qualification reader requests artifacts only from the pinned run', () => {
  const routes = [];
  const result = requireQualification(sha, { repository, runId: run.id, retain: value => value,
    readArchive: value => value, download: () => receipt, request: route => {
      routes.push(route);
      if (route.includes('/artifacts?')) return { artifacts: [artifact] };
      if (route.includes('/jobs?')) return { jobs: qualifiedJobs, total_count: qualifiedJobs.length };
      return run;
    } });
  assert.equal(result.run_id, run.id);
  assert.match(routes[0], /actions\/runs\/123\/artifacts\?/);
});

const actions = { listWorkflowRunArtifacts: async () => ({ data: { artifacts: [artifact] } }),
  getWorkflowRun: async () => ({ data: run }),
  listJobsForWorkflowRun: async () => ({ data: { jobs: qualifiedJobs, total_count: qualifiedJobs.length } }),
  getWorkflowRunAttempt: async () => ({ data: run }), downloadArtifact: async () => ({ data: receipt }) };
const context = { repo: { owner: 'domios-dev', repo: 'domios' }, eventName: 'pull_request',
  payload: { pull_request: { number: 7, state: 'open', base: { ref: 'master' }, user: { login: 'rollacode' },
    head: { sha, ref: branch, repo: { full_name: repository } } } } };

test('GitHub qualification reader pins both artifact lookup and provenance to the requested run', async () => {
  let lookup;
  const github = { rest: { actions: { ...actions,
    listWorkflowRunArtifacts: async args => { lookup = args; return { data: { artifacts: [artifact] } }; },
    listArtifactsForRepo: () => assert.fail('repository-wide lookup'),
  } } };
  const verified = await requireQualificationWithGithub(github, context, sha,
    { runId: run.id, readArchive: value => value, retain: value => value });
  assert.equal(lookup.run_id, run.id);
  assert.equal(verified.run_id, run.id);
  await assert.rejects(requireQualificationWithGithub(github, context, sha, { runId: 999 }), /No unexpired/);
});

test('hosted PR reader checks current release head and delegates the exact selected push run', async () => {
  let verification;
  const github = { rest: { actions: { listWorkflowRuns() {} },
    pulls: { get: async () => ({ data: context.payload.pull_request }) },
    repos: { getBranch: async () => ({ data: { commit: { sha } } }) } },
    paginate: async () => [run] };
  await requireReleasePrQualification(github, context, async (...args) => { verification = args; });
  assert.equal(verification[2], sha);
  assert.deepEqual(verification[3], { runId: run.id });
  for (const change of [{ user: { login: 'other' } }, { head: { ...context.payload.pull_request.head,
    repo: { full_name: 'fork/domios' } } }, { base: { ref: 'develop' } }])
    await assert.rejects(requireReleasePrQualification(github, { ...context,
      payload: { pull_request: { ...context.payload.pull_request, ...change } } }));
  await assert.rejects(requireReleasePrQualification({ ...github, paginate: async () => [] }, context));
  await assert.rejects(requireReleasePrQualification({ ...github, rest: { ...github.rest,
    repos: { getBranch: async () => ({ data: { commit: { sha: 'b'.repeat(40) } } }) } } }, context), /Superseded/);
  await assert.rejects(requireReleasePrQualification(github, { ...context, eventName: 'workflow_dispatch' }));
});

test('master promotion requires the qualified push graph including every platform and manifest', () => {
  const jobs = REQUIRED_JOBS.map(name => ({ name: 'Release packaging / ' + name, conclusion: 'success' }));
  assert.doesNotThrow(() => checkQualifiedRun({ run, jobs, sourceSha: sha, branch }));
  for (const changed of [{ event: 'pull_request' }, { head_branch: 'master' }, { head_sha: 'b'.repeat(40) },
    { conclusion: 'failure' }, { triggering_actor: { login: 'other' } }])
    assert.throws(() => checkQualifiedRun({ run: { ...run, ...changed }, jobs, sourceSha: sha, branch }));
  for (const job of jobs) assert.throws(() => checkQualifiedRun({ run, sourceSha: sha, branch,
    jobs: jobs.filter(row => row !== job) }));
});

test('CI result requires the complete release push graph and refuses a missing receipt', () => {
  const needs = Object.fromEntries(['policy', 'fast', 'rust', 'rust-warnings', 'qualification', 'release-dry-run']
    .map(id => [id, { result: 'success' }]));
  assert.deepEqual(ciFailures('push', needs, '', { releaseHead: true }), []);
  for (const event of ['pull_request', 'pull_request_target', 'workflow_dispatch'])
    assert.ok(ciFailures(event, needs, '', { releaseHead: true }).includes('admission'));
  for (const id of Object.keys(needs)) for (const result of ['skipped', 'failure', 'cancelled'])
    assert.ok(ciFailures('push', { ...needs, [id]: { result } }, '', { releaseHead: true }).includes(id));
  assert.ok(ciFailures('workflow_dispatch', needs, 'fast').includes('qualification'));
});

import { parseTestWaiver, verifyTestWaiver, waiverReleaseNotes } from './release-test-waiver.mjs';
import { releaseTag } from './release-version.mjs';
import { PLATFORMS } from './release-dry-run-manifest.mjs';

test('release/next takes its app tag from the exact package version', () => {
  assert.equal(releaseTag({ version: '1.20.3' }), 'v1.20.3');
  assert.equal(releaseTag({ version: '1.21.0' }, 'v1.21.0'), 'v1.21.0');
  assert.throws(() => releaseTag({ version: '1.21.0' }, 'v1.20.3'), /Expected tag v1.21.0/);
  assert.throws(() => releaseTag({ version: 'next' }), /Expected an X.Y.Z version/);
});

const testJob = 'Fast checks / ts unit (1/4)';
function waivedFixture() {
  const failedRun = { ...run, conclusion: 'failure', updated_at: new Date().toISOString() };
  const jobs = structuredClone(qualifiedJobs);
  const fail = (job, step) => {
    job.conclusion = 'failure';
    job.steps[0] = { ...job.steps[0], name: step, conclusion: 'failure' };
  };
  const unit = jobs.find(job => job.name === 'Fast checks / ts unit');
  unit.name = testJob;
  fail(unit, 'Run TS unit tests');
  fail(jobs.find(job => job.name === 'CI result'), 'Require the complete event graph');
  Object.assign(jobs.find(job => job.name === 'qualification receipt'), { conclusion: 'skipped', steps: [] });
  for (const name of ['rust-warnings / report', ...REQUIRED_JOBS.map(name => 'Release packaging / ' + name + (name.endsWith(' / ') ? 'build and sign' : ''))]) {
    jobs.push({ ...structuredClone(qualifiedJobs[0]), id: jobs.length + 1, name });
  }
  const tag = 'v1.20.0';
  const receipts = PLATFORMS.map(platform => ({ schema: 1, platform, source_sha: sha, tag,
    files: [{ name: platform+'.bin', sha256: 'c'.repeat(64), size: 42 }] }));
  const assets = receipts.flatMap((receipt, index) => [asset(receipt.files[0]),
    { id: 100+index, name: `qualification-${receipt.platform}.json`, state: 'uploaded' }]);
  return { waiver: { run_id: String(run.id), jobs: [testJob] }, run: failedRun, jobs, sha, repository, tag, attempts: [failedRun],
    draft: { id: 10, draft: true, tag_name: `qualification-${tag}-${run.id}`, target_commitish: sha,
      body: `<!-- qualification:${JSON.stringify({ source_sha: sha, run_id: String(run.id) })} -->\n<!-- qualification-complete -->` },
    assets, receipts };
}

test('explicit test waiver admits the failed exact run with six green builds and matching hidden draft assets', () => {
  const input = waivedFixture();
  const result = verifyTestWaiver(input);
  assert.deepEqual(result.test_waiver.jobs, [testJob]);
  assert.deepEqual(result.test_waiver.derived_jobs, [
    { name: 'CI result', conclusion: 'failure' }, { name: 'qualification receipt', conclusion: 'skipped' }]);
  assert.equal(result.run_id, run.id);
  assert.equal(selectReleasePushRun([input.run], sha, branch, repository, input.waiver).id, run.id);
  assert.throws(() => selectReleasePushRun([input.run], sha, branch, repository));
  assert.throws(() => selectReleasePushRun([input.run, { ...input.run, id: 124 }], sha, branch, repository, input.waiver));
});

test('waivers refuse build/sign names, aggregate names, unknown names, duplicates, wrong runs and non-failing tests', () => {
  for (const name of ['Release packaging / Windows arm64 / build and sign', 'Fast checks / frontend producer',
    'CI result', 'unknown test']) {
    const input = waivedFixture();
    input.waiver.jobs = [name];
    assert.throws(() => verifyTestWaiver(input), /only exact test jobs/);
  }
  for (const change of [{ jobs: [testJob, testJob] }, { run_id: '124' }, { run_id: 123 },
    { jobs: ['Fast checks / ts vitest'] }]) {
    const input = waivedFixture();
    input.waiver = { ...input.waiver, ...change };
    assert.throws(() => verifyTestWaiver(input));
  }
});

test('unnamed failures, incomplete builds, cancellation, foreign authority and missing or changed assets refuse', () => {
  const mutations = [
    input => { input.jobs.find(job => job.name === 'Fast checks / ts vitest').conclusion = 'failure'; },
    input => { input.jobs.find(job => job.name.startsWith('Release packaging / Windows arm64')).conclusion = 'failure'; },
    input => { input.jobs.find(job => job.name.startsWith('Release packaging / macOS x64')).conclusion = 'skipped'; },
    input => { input.jobs = input.jobs.filter(job => !job.name.startsWith('Release packaging / Linux arm64')); },
    input => { input.jobs.find(job => job.name === 'CI result').steps[0].name = 'Checkout'; },
    input => { input.run.conclusion = 'cancelled'; },
    input => { input.run.actor = { login: 'other' }; },
    input => { input.run.head_sha = 'b'.repeat(40); },
    input => { input.run.updated_at = '2000-01-01T00:00:00Z'; },
    input => { input.draft = null; },
    input => { input.draft.draft = false; },
    input => { input.draft.body = input.draft.body.replace('qualification-complete', 'partial'); },
    input => { input.assets.splice(0, 1); },
    input => { input.assets[0].digest = 'sha256:'+'d'.repeat(64); },
    input => { input.receipts.pop(); },
  ];
  for (const mutate of mutations) {
    const input = waivedFixture();
    mutate(input);
    assert.throws(() => verifyTestWaiver(input), mutate.toString());
  }
});

test('Rust waiver derives only dependency failures and the named tests incomplete warning collections', () => {
  const input = waivedFixture();
  const unit = input.jobs.find(job => job.name === testJob);
  unit.conclusion = unit.steps[0].conclusion = 'success';
  const full = input.jobs.find(job => job.name === 'Rust tests / rust tests (full suite)');
  input.waiver.jobs = [full.name];
  for (const [name, step] of [[full.name, 'Run full Rust tests (Unix)'],
    ['Rust tests / rust test receipts', 'Require every Rust test job'],
    ['rust-warnings / report', 'Report advisory warnings and verify collection completeness']]) {
    const job = input.jobs.find(job => job.name === name);
    job.conclusion = job.steps[0].conclusion = 'failure';
    job.steps[0].name = step;
  }
  input.warningLog = '2026-11-16T00:00:00Z Rust warning evidence incomplete: tests-full-Linux-x86_64-unknown-linux-gnu\n';
  assert.equal(verifyTestWaiver(input).test_waiver.derived_jobs.length, 4);
  for (const warningLog of ['', input.warningLog+'Rust warning evidence missing: release-Windows\n',
    input.warningLog.replace('tests-full-Linux-x86_64-unknown-linux-gnu', 'release-Linux-x86_64-unknown-linux-gnu')]) {
    assert.throws(() => verifyTestWaiver({ ...input, warningLog }), /Warning report/);
  }
  full.steps[0].name = 'Compile full Rust tests (Unix)';
  assert.throws(() => verifyTestWaiver(input), /Unrelated failure/);
});

function waiverRequest(input, route) {
  if (route === `repos/${repository}/contents/package.json?ref=${sha}`) {
    return { encoding: 'base64', content: Buffer.from(JSON.stringify({ version: input.tag.slice(1) })).toString('base64') };
  }
  if (route.includes('/jobs?')) return { jobs: input.jobs, total_count: input.jobs.length };
  if (route.endsWith(`/releases/tags/qualification-${input.tag}-${input.run.id}`)) return input.draft;
  if (route.includes('/releases/10/assets?')) return input.assets;
  const assetId = /\/releases\/assets\/(\d+)$/.exec(route);
  if (assetId) return JSON.stringify(input.receipts[Number(assetId[1])-100]);
  if (/\/actions\/runs\/123(?:\/attempts\/1)?$/.test(route)) return input.run;
  assert.fail('unexpected request: '+route);
}

test('CLI and hosted PR waiver readers verify direct run evidence without requiring a skipped receipt artifact', async () => {
  const input = waivedFixture();
  const cli = requireQualification(sha, { repository, runId: run.id, waiver: input.waiver,
    request: route => waiverRequest(input, route), retain: value => value });
  assert.deepEqual(cli.test_waiver.jobs, [testJob]);
  const github = { request: async route => ({ data: waiverRequest(input, route.slice(5)) }) };
  const hosted = await requireQualificationWithGithub(github, context, sha, { runId: run.id, waiver: input.waiver, retain: value => value });
  assert.deepEqual(hosted, cli);
  await assert.rejects(requireQualificationWithGithub(github, context, sha, { runId: 999, waiver: input.waiver }), /different run/);
});

test('owner PR field pins exact test names and release notes keep that decision visible and idempotent', async () => {
  const input = waivedFixture();
  const body = 'Release notes\nRelease-Test-Waiver: '+JSON.stringify(input.waiver);
  assert.deepEqual(parseTestWaiver(body), input.waiver);
  assert.equal(parseTestWaiver('No waiver'), undefined);
  assert.throws(() => parseTestWaiver(body+'\n'+body), /Ambiguous/);
  const notes = waiverReleaseNotes('Substantive release notes.', input.waiver);
  assert.match(notes, /qualification run 123/);
  assert.ok(notes.includes(testJob));
  assert.equal(waiverReleaseNotes(notes, input.waiver), notes);
  let selected;
  const github = { rest: { actions: { listWorkflowRuns() {} },
    pulls: { get: async () => ({ data: { ...context.payload.pull_request, body } }) },
    repos: { getBranch: async () => ({ data: { commit: { sha } } }) } }, paginate: async () => [input.run] };
  await requireReleasePrQualification(github, context, async (...args) => { selected = args[3]; });
  assert.deepEqual(selected, { runId: run.id, waiver: input.waiver });
});

test('publication retries require an already recorded identical waiver; PR admission still requires a hidden draft', () => {
  const input = waivedFixture();
  const admission = verifyTestWaiver(input);
  assert.doesNotThrow(() => checkQualifiedRun({ run: input.run, jobs: input.jobs, sourceSha: sha, branch, waiver: admission.test_waiver }));
  assert.throws(() => checkQualifiedRun({ run: { ...input.run, conclusion: 'cancelled' }, jobs: input.jobs,
    sourceSha: sha, branch, waiver: admission.test_waiver }));
  assert.throws(() => checkQualifiedRun({ run: input.run, jobs: input.jobs, sourceSha: sha, branch, waiver: input.waiver }));
  input.draft.draft = false;
  input.draft.tag_name = 'v1.20.0';
  assert.throws(() => verifyTestWaiver({ ...input, admission }), /hidden draft/);
  assert.doesNotThrow(() => verifyTestWaiver({ ...input, publicationRecovery: true, admission }));
  for (const change of [{ run_id: 999 }, { sha: 'b'.repeat(40) }, { run_attempt: 2 },
    { test_waiver: { ...admission.test_waiver, jobs: ['Fast checks / ts vitest'] } }]) {
    assert.throws(() => verifyTestWaiver({ ...input, publicationRecovery: true, admission: { ...admission, ...change } }), /hidden draft/);
  }
  assert.throws(() => verifyTestWaiver({ ...input, publicationRecovery: true }), /hidden draft/);
});

test('only one versioned plugin and its owned files qualify for lightweight CI', () => {
  const before = { plugins: [
    { id: 'opencode', path: 'provider-opencode', version: '1.14.28', minCodeterm: '1.20.2' },
    { id: 'codex', path: 'provider-codex', version: '1.14.80', minCodeterm: '1.20.2' },
  ] };
  const after = structuredClone(before);
  after.plugins[0].version = '1.14.29';
  const paths = ['plugins/channel.json', 'plugins/provider-opencode/plugin.json', 'tests/fixtures/runtime-http-sse.json'];
  assert.deepEqual(classifyPluginDiff(paths, before, after), { id: 'opencode', tag: 'plugin-opencode-v1.14.29' });
  for (const extra of ['package.json', 'tests/fixtures/foreign.json', 'plugins/provider-codex/plugin.json',
    'plugins/provider-opencode-other/plugin.json', '.github/workflows/tests.yml']) {
    assert.equal(classifyPluginDiff([...paths, extra], before, after), null);
  }
  assert.equal(classifyPluginDiff(paths, before, before), null);
  after.plugins[1].version = '1.14.81';
  assert.equal(classifyPluginDiff(paths, before, after), null);
});

test('direct plugin PR naming identifies the one expected plugin', () => {
  assert.equal(directPluginId('release/plugin-opencode'), 'opencode');
  assert.equal(directPluginId('release/plugin-chat-preprocessor'), 'chat-preprocessor');
  for (const ref of ['release/next', 'release/plugin-', 'release/plugin-opencode/topic', 'feature/plugin-opencode']) {
    assert.equal(directPluginId(ref), null);
  }
});
