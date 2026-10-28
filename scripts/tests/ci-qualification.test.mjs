import assert from 'node:assert/strict';
import test from 'node:test';
import { QUALIFICATION_JOBS, verifyQualification, selectQualificationArtifact, requireQualification, recordQualification } from '../ci-qualification.mjs';
import { requireQualificationWithGithub } from '../ci-workflow-api.mjs';

const sha = 'a'.repeat(40);
const repository = 'fixture/source';
function fixture() {
  const run = { id: 42, run_attempt: 2, head_sha: sha, head_branch: 'release/1.16.1',
    event: 'pull_request', path: '.github/workflows/tests.yml', status: 'completed', conclusion: 'success',
    repository: { full_name: repository }, head_repository: { full_name: repository } };
  const jobs = QUALIFICATION_JOBS.map((name, i) => ({ id: i + 1, name, run_id: 42,
    runner_name: name.includes('(macOS)') || name.endsWith('embeddings') ? 'macOS' : name.includes('(Windows)') ? 'Windows' : 'Linux',
    head_sha: sha, run_attempt: 1, status: 'completed', conclusion: 'success' }));
  const rust = [['full', 'Linux'], ['platform', 'macOS'], ['platform', 'Windows'], ['embeddings', 'macOS']]
    .map(([mode, os]) => ({ mode, os, sha, run_id: '42', repository, run_attempt: 1,
      producer: mode === 'full' ? 'rust-tests' : mode === 'platform' ? 'rust-platform' : 'rust-embeddings',
      executed: 7, exit: 0, seconds: 1, runner: os }));
  const receipt = { version: 2, sha, run_id: '42', run_attempt: 1, repository,
    workflow: run.path, results: { fast: 'success', rust: 'success', warnings: 'success' }, rust };
  return { run, jobs, receipt };
}

test('failed-only publication rerun inherits attributable green jobs and receipt', () => {
  const f = fixture();
  assert.notEqual(f.receipt.run_attempt, f.run.run_attempt);
  assert.equal(f.jobs.filter(job => job.run_attempt === f.run.run_attempt).length, 0);
  f.jobs.push({ ...f.jobs[1], id: 101, run_attempt: 2 });
  const verified = verifyQualification(f.receipt, f.run, f.jobs, sha, repository);
  assert.equal(verified.verified_run_attempt, 2);
  assert.equal(verified.verified_jobs.find(job => job.name === 'CI result').run_attempt, 2);
  assert.equal(verified.verified_jobs.find(job => job.name === 'qualification receipt').run_attempt, 1);
});

test('later qualification recording retains real earlier Rust producers', () => {
  const f = fixture();
  f.run.run_attempt = 3;
  f.jobs.push({ ...f.jobs.at(-1), id: 102, run_attempt: 3 });
  assert.throws(() => verifyQualification(f.receipt, f.run, f.jobs, sha, repository), /receipt/);
  f.receipt = recordQualification(sha, f.receipt.rust, f.receipt.results, { repository, run_id: '42', run_attempt: 3 });
  assert.equal(verifyQualification(f.receipt, f.run, f.jobs, sha, repository).run_attempt, 3);
});

for (const [name, mutate] of [
  ['wrong source SHA', f => { f.run.head_sha = 'b'.repeat(40); }],
  ['wrong receipt run', f => { f.receipt.run_id = '43'; }],
  ['foreign repository', f => { f.run.repository.full_name = 'foreign/source'; }],
  ['foreign workflow', f => { f.run.path = '.github/workflows/foreign.yml'; }],
  ['future receipt', f => { f.receipt.run_attempt = 3; }],
  ['missing job', f => { f.jobs.shift(); }],
  ['skipped job', f => { f.jobs[0].conclusion = 'skipped'; }],
  ['failed latest execution', f => { f.jobs.push({ ...f.jobs[0], id: 111, run_attempt: 2, conclusion: 'failure' }); }],
  ['foreign job', f => { f.jobs[0].run_id = 43; }],
  ['foreign job SHA', f => { f.jobs[0].head_sha = 'b'.repeat(40); }],
  ['stale Rust receipt', f => { f.jobs.push({ ...f.jobs[9], id: 112, run_attempt: 2 }); }],
  ['foreign Rust receipt', f => { f.receipt.rust[0].run_id = '43'; }],
  ['foreign Rust producer', f => { f.receipt.rust[0].producer = 'other-job'; }],
  ['foreign Rust runner', f => { f.receipt.rust[0].runner = 'other-runner'; }],
  ['foreign receipt repository', f => { f.receipt.repository = 'foreign/source'; }],
  ['foreign receipt workflow', f => { f.receipt.workflow = '.github/workflows/other.yml'; }],
  ['failed qualification run', f => { f.run.conclusion = 'failure'; }],
  ['foreign receipt SHA', f => { f.receipt.sha = 'b'.repeat(40); }],
]) test(name + ' refuses', () => {
  const f = fixture(); mutate(f);
  assert.throws(() => verifyQualification(f.receipt, f.run, f.jobs, sha, repository));
});

test('recording refuses incomplete stages or foreign same-SHA Rust evidence', () => {
  const f = fixture(), identity = { repository, run_id: '42', run_attempt: 3 };
  assert.throws(() => recordQualification(sha, f.receipt.rust, {}, identity));
  f.receipt.rust[0].run_id = '43';
  assert.throws(() => recordQualification(sha, f.receipt.rust, f.receipt.results, identity));
});

test('expired and foreign artifacts refuse before download', () => {
  const row = { id: 8, name: `qualification-${sha}`, expired: false,
    expires_at: '2026-10-28T00:00:00Z', workflow_run: { id: 42, head_sha: sha } };
  const now = Date.parse('2026-10-27T00:00:00Z');
  assert.equal(selectQualificationArtifact([row], sha, now), row);
  for (const artifact of [{ ...row, expired: true }, { ...row, expires_at: '2026-10-26T00:00:00Z' },
    { ...row, workflow_run: { id: 43, head_sha: 'b'.repeat(40) } }]) {
    assert.throws(() => selectQualificationArtifact([artifact], sha, now));
  }
});

test('CLI adapter downloads the selected artifact ID and paginates all job attempts', () => {
  const f = fixture(), calls = [];
  const artifact = { id: 81, name: `qualification-${sha}`, expired: false,
    expires_at: '2999-01-01T00:00:00Z', workflow_run: { id: 42, head_sha: sha } };
  const responses = {
    [`repos/${repository}/actions/artifacts?name=qualification-${sha}&per_page=100`]: { artifacts: [artifact] },
    [`repos/${repository}/actions/runs/42`]: f.run,
    [`repos/${repository}/actions/runs/42/jobs?filter=all&per_page=100&page=1`]: { total_count: f.jobs.length, jobs: f.jobs.slice(0, 8) },
    [`repos/${repository}/actions/runs/42/jobs?filter=all&per_page=100&page=2`]: { total_count: f.jobs.length, jobs: f.jobs.slice(8) },
  };
  const result = requireQualification(sha, { repository, request: route => {
    calls.push(route); assert.ok(Object.hasOwn(responses, route), route); return responses[route];
  }, download: args => { assert.deepEqual(args, ['api', `repos/${repository}/actions/artifacts/81/zip`]); return 'archive'; },
  readArchive: archive => { assert.equal(archive, 'archive'); return f.receipt; }, retain: receipt => receipt });
  assert.equal(result.verified_jobs.length, QUALIFICATION_JOBS.length);
  assert.equal(calls.length, 4);
});

test('workflow adapter uses the same all-attempt evidence and exact artifact', async () => {
  const f = fixture(), calls = [];
  const github = { rest: { actions: {
    listArtifactsForRepo: async () => ({ data: { artifacts: [{ id: 81, name: `qualification-${sha}`, expired: false,
      expires_at: '2999-01-01T00:00:00Z', workflow_run: { id: 42, head_sha: sha } }] } }),
    getWorkflowRun: async args => { assert.equal(args.run_id, 42); return { data: f.run }; },
    listJobsForWorkflowRun: async args => {
      calls.push(args); assert.equal(args.filter, 'all'); assert.equal(args.run_id, 42);
      return { data: { total_count: f.jobs.length, jobs: args.page === 1 ? f.jobs.slice(0, 8) : f.jobs.slice(8) } };
    },
    downloadArtifact: async args => { assert.equal(args.artifact_id, 81); return { data: 'archive' }; },
  } } };
  const result = await requireQualificationWithGithub(github, { repo: { owner: 'fixture', repo: 'source' } }, sha, () => f.receipt, receipt => receipt);
  assert.equal(result.verified_run_attempt, 2);
  assert.equal(calls.length, 2);
});
