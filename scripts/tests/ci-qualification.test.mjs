import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { QUALIFICATION_JOBS, requireQualification, verifyQualification } from '../ci-qualification.mjs';

const sha = 'a'.repeat(40);
const run = { id: 17, run_attempt: 2, path: '.github/workflows/tests.yml', event: 'push', head_branch: 'develop', head_sha: sha, status: 'completed', conclusion: 'success' };
const jobs = () => QUALIFICATION_JOBS.map(name => ({ name, conclusion: 'success' }));
const receipt = () => ({
  version: 1, sha, run_id: '17', run_attempt: 2, results: { fast: 'success', rust: 'success', warnings: 'success' },
  rust: [['full', 'Linux'], ['platform', 'macOS'], ['platform', 'Windows'], ['embeddings', 'macOS']]
    .map(([mode, os]) => ({ mode, os, sha, exit: 0, executed: 1, seconds: 1 })),
});

test('qualification accepts only exact-SHA green dev evidence with every distinct oracle', () => {
  assert.equal(verifyQualification(receipt(), run, jobs(), sha).sha, sha);
  for (const [field, value] of [['path', '.github/workflows/release.yml'], ['conclusion', 'failure'], ['status', 'in_progress'], ['event', 'pull_request'], ['head_branch', 'lane/opaque'], ['head_sha', 'b'.repeat(40)], ['run_attempt', 3]]) {
    assert.throws(() => verifyQualification(receipt(), { ...run, [field]: value }, jobs(), sha), undefined, field);
  }
  for (const prefix of QUALIFICATION_JOBS) {
    assert.throws(() => verifyQualification(receipt(), run, jobs().filter(job => job.name !== prefix), sha), /Missing green/, prefix);
    for (const conclusion of ['skipped', 'cancelled', 'failure']) {
      assert.throws(() => verifyQualification(receipt(), run, jobs().map(job => job.name === prefix ? { ...job, conclusion } : job), sha), /Missing green/, `${prefix}/${conclusion}`);
    }
  }
  for (const mutate of [
    row => { row.sha = 'b'.repeat(40); }, row => { row.run_id = 18; }, row => { row.run_attempt = 1; },
    row => { delete row.results.fast; }, row => { row.results.rust = 'skipped'; }, row => { row.results.warnings = 'failure'; },
    row => { row.rust[0].sha = 'b'.repeat(40); }, row => { row.rust[0].executed = 0; },
    row => { row.rust[1].exit = 1; }, row => { row.rust.pop(); }, row => { row.rust[0].os = 'Windows'; },
  ]) {
    const value = receipt(); mutate(value);
    assert.throws(() => verifyQualification(value, run, jobs(), sha));
  }
});

test('nightly evidence pins the tested SHA independently of the default branch scheduler SHA', () => {
  verifyQualification(receipt(), { ...run, event: 'schedule', head_sha: 'b'.repeat(40) }, jobs(), sha);
  assert.throws(() => verifyQualification(receipt(), { ...run, event: 'workflow_dispatch' }, jobs(), sha), /Untrusted/);
});

test('release push qualification requires a complete version branch and the exact source SHA', () => {
  for (const branch of ['release/1.15.3', 'release/20.0.12']) {
    assert.equal(verifyQualification(receipt(), { ...run, head_branch: branch }, jobs(), sha).sha, sha);
    assert.throws(() => verifyQualification(receipt(), { ...run, head_branch: branch, head_sha: 'b'.repeat(40) }, jobs(), sha), /Untrusted/);
  }
  for (const branch of ['release/1.15', 'release/1.15.3-extra', 'release/1.15.3/fix', 'release/latest', 'release/x.y.z', 'lane/release/1.15.3']) {
    assert.throws(() => verifyQualification(receipt(), { ...run, head_branch: branch }, jobs(), sha), /Untrusted/, branch);
  }
});

test('receipt lookup is bounded, refuses missing evidence and rejects a stale artifact from an earlier attempt', () => {
  assert.throws(() => requireQualification(sha, { request: () => ({ artifacts: [] }) }), /No qualification receipt/);
  assert.throws(() => requireQualification('HEAD'), /Invalid qualification SHA/);
  const request = route => {
    if (route.includes('/artifacts?')) return { artifacts: [{ id: 20, name: `qualification-${sha}`, workflow_run: { id: 17 } }] };
    if (route.includes('/jobs?')) return { jobs: jobs(), total_count: jobs().length };
    return run;
  };
  const download = value => fs.writeFileSync(path.join(value.at(-1), 'qualification.json'), JSON.stringify(receipt()));
  assert.equal(requireQualification(sha, { request, download }).run_id, '17');
  assert.throws(() => requireQualification(sha, { request, download: value => {
    fs.writeFileSync(path.join(value.at(-1), 'qualification.json'), JSON.stringify({ ...receipt(), run_attempt: 1 }));
  } }), /stale/);
});
