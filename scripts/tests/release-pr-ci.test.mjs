import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { ciFailures } from '../ci-result.mjs';
import { verifyQualification, QUALIFICATION_JOBS } from '../ci-qualification.mjs';
import { isReleasePushRun } from '../release-promote.mjs';

const workflow = name => parse(fs.readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const sha = 'a'.repeat(40);
const repository = { full_name: 'domios-dev/domios', owner: { login: 'domios-dev' } };
const run = {
  id: 17, run_attempt: 1, path: '.github/workflows/tests.yml', event: 'pull_request', head_branch: 'release/1.16.0',
  head_sha: sha, status: 'completed', conclusion: 'success', repository, head_repository: { full_name: repository.full_name },
  actor: { login: 'rollacode' }, triggering_actor: { login: 'rollacode' },
};


test('the release PR graph requires guards, every tier, qualification and packaging', () => {
  const graph = Object.fromEntries(['policy', 'fast', 'rust', 'rust-warnings', 'qualification', 'release-dry-run'].map(id => [id, { result: 'success' }]));
  assert.deepEqual(ciFailures('pull_request', graph, '', { releaseHead: true }), []);
  for (const id of Object.keys(graph)) {
    assert.ok(ciFailures('pull_request', { ...graph, [id]: { result: 'skipped' } }, '', { releaseHead: true }).includes(id), id);
  }
});

test('qualification and promotion accept only the same-repository release PR head', () => {
  const receipt = {
    version: 1, sha, run_id: '17', run_attempt: 1, results: { fast: 'success', rust: 'success', warnings: 'success' },
    rust: [['full', 'Linux'], ['platform', 'macOS'], ['platform', 'Windows'], ['embeddings', 'macOS']]
      .map(([tier, platform]) => ({ sha, tier, platform })),
  };
  const jobs = QUALIFICATION_JOBS.map(name => ({ name, conclusion: 'success' }));
  assert.throws(() => verifyQualification(receipt, { ...run, head_repository: { full_name: 'fork/domios' } }, jobs, sha), /Untrusted/);
  assert.throws(() => verifyQualification(receipt, { ...run, event: 'schedule' }, jobs, sha), /Untrusted/);
  assert.equal(isReleasePushRun(run, sha), true);
  for (const bad of [{ ...run, event: 'push' }, { ...run, head_sha: 'b'.repeat(40) }, { ...run, head_repository: { full_name: 'fork/domios' } }]) {
    assert.equal(isReleasePushRun(bad, sha), false);
  }
});
