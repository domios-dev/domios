import { condition as workflowCondition } from './workflow-condition.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { parse } from 'yaml';
import { headlessAssetNames } from '../headless-release-package.mjs';
import { checkQualifiedRun, promotionPlan, publishPlatform, REQUIRED_JOBS } from '../release-promote.mjs';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const release = workflow('release');
const sha = 'b'.repeat(40);
const branch = 'release/9.9.9';
const qualifiedJobs = () => [
  'prepare release', 'Fast checks / guards', 'Fast checks / clippy (linux)', 'Fast checks / ts unit', 'Fast checks / ts vitest',
  'Fast checks / rust tests (full suite)', 'Rust tests / rust tests (full suite) (full)', 'Rust tests / rust platform (Linux)',
  'Rust tests / rust platform (Windows)', 'macOS arm64 / ', 'Rust tests / rust embeddings (embeddings)',
  'Rust tests / rust test receipts', 'macOS arm64 / build (arm64, aarch64, macos-ci)', 'macOS x64 / build (x86_64, x64, macos-ci)',
  'Linux arm64 / build', 'Linux x64 / build (x86_64-unknown-linux-gnu)', 'Windows x64 / build (x86_64-pc-windows-msvc)',
  'Windows arm64 / build (aarch64-pc-windows-msvc)', 'local dry-run manifest verification / verify consolidated manifests against private CI bytes',
  'release manifest, mirror, and verification',
].map(name => ({ name, conclusion: /rust tests \(full suite\)$|^release manifest/.test(name) ? 'skipped' : 'success' }));
const qualifiedRun = { id: 41, path: '.github/workflows/release.yml', status: 'completed', conclusion: 'success', head_sha: sha, head_branch: branch };

test('only a green release.yml dry run of the exact branch head with every gate qualifies', () => {
  checkQualifiedRun({ run: qualifiedRun, jobs: qualifiedJobs(), sourceSha: sha, branch });
  const refusals = [
    [{ ...qualifiedRun, head_sha: 'c'.repeat(40) }, qualifiedJobs(), /built c+, not b+/],
    [{ ...qualifiedRun, head_branch: 'develop' }, qualifiedJobs(), /ran on develop/],
    [{ ...qualifiedRun, conclusion: 'cancelled' }, qualifiedJobs(), /completed\/cancelled/],
    [{ ...qualifiedRun, path: '.github/workflows/tests.yml' }, qualifiedJobs(), /not a release\.yml run/],
    [qualifiedRun, qualifiedJobs().filter(job => !job.name.startsWith('Windows arm64')), /has no Windows arm64 job/],
    [qualifiedRun, qualifiedJobs().map(job => job.name.startsWith('macOS arm64 / ') ? { ...job, conclusion: 'skipped' } : job),
      /macOS arm64 .* is skipped/],
    [qualifiedRun, qualifiedJobs().map(job => job.name.startsWith('local dry-run') ? { ...job, conclusion: 'failure' } : job), /is failure/],
  ];
  for (const [run, jobs, message] of refusals) {
    assert.throws(() => checkQualifiedRun({ run, jobs, sourceSha: sha, branch }), message);
  }
});

test('every required job name exists in the release graph', () => {
  const callers = Object.values(release.jobs);
  for (const required of REQUIRED_JOBS) {
    const [caller, inner] = required.split(' / ');
    const job = callers.find(entry => entry.name === caller || (inner === undefined && caller.startsWith(entry.name ?? '\0')));
    assert.ok(job, required);
    if (inner) {
      const called = workflow(path.basename(job.uses, '.yml'));
      assert.ok(Object.values(called.jobs).some(entry => (entry.name ?? '').startsWith(inner.replace(/ \(.*$/, '')) || inner === ''), required);
    }
  }
});

test('the source draft takes every retained distributable and fragment; the mirror takes no fragment', () => {
  const tag = 'v9.9.9';
  const headless = headlessAssetNames(tag, 'darwin-aarch64');
  const receipt = { files: ['Domios_9.9.9_aarch64.dmg', 'Domios_aarch64.app.tar.gz', 'updater-darwin-aarch64.json',
    headless.archive, headless.checksum, headless.manifest].map(name => ({ name })) };
  assert.deepEqual(promotionPlan(tag, 'darwin-aarch64', receipt), {
    source: ['Domios_9.9.9_aarch64.dmg', 'Domios_aarch64.app.tar.gz', 'updater-darwin-aarch64.json'],
    mirror: ['Domios_9.9.9_aarch64.dmg', 'Domios_aarch64.app.tar.gz'],
  });
});

test('promotion uploads the retained bytes, reads each back and refuses a changed asset', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'promote-fixture-'));
  try {
    const tag = 'v9.9.9';
    const names = ['Domios_9.9.9_amd64.AppImage', 'Domios_9.9.9_amd64.deb', 'updater-linux-x86_64.json', 'updater-linux-x86_64-deb.json'];
    const files = [];
    for (const name of names) {
      await writeFile(path.join(directory, name), 'bytes of ' + name);
      files.push({ name, sha256: createHash('sha256').update('bytes of ' + name).digest('hex') });
    }
    const receipt = { files };
    const calls = [];
    const headless = [];
    let tamper = '';
    const run = args => {
      calls.push(args);
      if (args[1] !== 'download') return;
      const name = args[args.indexOf('--pattern') + 1];
      const target = path.join(args[args.indexOf('--dir') + 1], name);
      copyFileSync(path.join(directory, name), target);
      if (name === tamper) writeFileSync(target, 'served something else');
    };
    const options = { tag, sourceSha: sha, platform: 'linux-x86_64', directory, receipt, env: {}, run,
      publishHeadless: options => headless.push(options.repository) };
    assert.deepEqual(await publishPlatform({ ...options, repository: 'rollacode/codeterm-releases' }), names.slice(0, 2));
    assert.deepEqual(calls[0].slice(0, 3), ['release', 'upload', tag]);
    assert.equal(calls[0].filter(arg => arg.includes('updater-')).length, 0);
    assert.equal(calls.filter(args => args[1] === 'download').length, 2);
    assert.deepEqual(await publishPlatform({ ...options, repository: 'rollacode/codeterm' }), names);
    assert.deepEqual(headless, ['rollacode/codeterm-releases', 'rollacode/codeterm']);
    tamper = 'Domios_9.9.9_amd64.deb';
    await assert.rejects(publishPlatform({ ...options, repository: 'rollacode/codeterm' }),
      /rollacode\/codeterm Domios_9\.9\.9_amd64\.deb differs from its linux-x86_64 receipt/);
    assert.equal(headless.length, 2, 'a refused platform never reaches headless publication');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('a promotion run publishes without building, testing or a second warning report', () => {
  const github = { repository_owner: 'rollacode', actor: 'rollacode', triggering_actor: 'rollacode',
    event_name: 'workflow_dispatch', ref: 'refs/heads/release/9.9.9', event: { workflow_run: {} } };
  const evaluate = expression => (needs, vars) => workflowCondition(expression, { needs, vars, github });
  const prepare = { result: 'success', outputs: { promote_run_id: '41', published: 'false', kind: 'app', dry_run: 'false', run_tests: 'false' } };
  assert.equal(evaluate(release.jobs['frontend-dist'].if)({ prepare }, {}), false);
  for (const id of ['macos-arm64', 'macos-x64', 'linux-arm64', 'linux-x86_64', 'windows-x64', 'windows-arm64', 'rust-warnings']) {
    // Even a successful bundle job could not start a build in a promotion run.
    const needs = { prepare, 'quick-checks': { result: 'skipped' }, 'frontend-dist': { result: 'success' }, 'builds-tier': { result: 'skipped' } };
    assert.equal(evaluate(release.jobs[id].if)(needs, {}), false, id);
  }
  assert.equal(release.jobs['quick-checks'], undefined);
  const vars = { RELEASE_PUBLISH_ENABLED: 'true' };
  assert.equal(evaluate(release.jobs['promote-evidence'].if)({ prepare }, vars), true);
  assert.equal(evaluate(release.jobs['promote-evidence'].if)({ prepare }, {}), false, 'publication stays behind the owner switch');
  const skipped = { result: 'skipped' };
  const builds = result => Object.fromEntries(['macos-arm64', 'macos-x64', 'linux-arm64', 'linux-x86_64', 'windows-x64', 'windows-arm64', 'tests', 'quick-checks']
    .map(id => [id, { result }]));
  const manifest = evaluate(release.jobs['release-manifest'].if);
  assert.equal(manifest({ prepare, ...builds('skipped'), 'promote-evidence': { result: 'success' } }, vars), true);
  assert.equal(manifest({ prepare, ...builds('skipped'), 'promote-evidence': { result: 'failure' } }, vars), false);
  const built = { result: 'success', outputs: { ...prepare.outputs, promote_run_id: '' } };
  assert.equal(manifest({ prepare: built, ...builds('success'), 'promote-evidence': skipped }, vars), true);
  assert.equal(manifest({ prepare: built, ...builds('success'), 'windows-arm64': { result: 'failure' }, 'promote-evidence': skipped }, vars), false);
});

test('the promotion CLI calls only gh flags every runner build has', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = await readFile(new URL('../release-promote.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /'--slurp'/);
});
