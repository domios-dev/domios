#!/usr/bin/env node
import { sourceRepository } from './source-repository.mjs';
// Caller: CI release.yml (prepare, promote-evidence); skill github-release
// Promotes retained packaging from the qualified release push run without rebuilding.
//
//   release-promote.mjs check <run-id> <source-sha> <release-branch>
//   release-promote.mjs publish <tag> <source-sha> <run-id> <work-dir>
import { requireQualification, isReleasePushRun } from './ci-qualification.mjs';
import { execFile, execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HEADLESS_TARGETS } from './headless-release-package.mjs';
import { promotionPlan, verifyAssetDigests } from './release-asset-inventory.mjs';

export { promotionPlan, verifyAssetDigests } from './release-asset-inventory.mjs';

const SOURCE_REPO = sourceRepository();
// 1.19 transition mirror: PUBLISH_MIRROR also writes this repository; delete after 1.19 ships.
const MIRROR_REPO = 'rollacode/codeterm-releases';
const exec = promisify(execFile);
export const PUBLICATION_CONCURRENCY = 6;
const PLATFORMS = Object.keys(HEADLESS_TARGETS).sort();

async function fetchAndAggregate(options) {
  const manifest = await import('./release-dry-run-manifest.mjs');
  return manifest.fetchAndAggregate(options);
}

// Each name prefixes a release.yml job (reusable callers add " / <job>") that must have succeeded.
export const REQUIRED_JOBS = [
  'prepare release',
  'macOS arm64 / ', 'macOS x64 / ', 'Linux arm64 / ', 'Linux x64 / ', 'Windows x64 / ', 'Windows arm64 / ',
  'local dry-run manifest verification / ',
];

function refuse(message) {
  throw new Error('promotion refused: ' + message);
}

export { isReleasePushRun } from './ci-qualification.mjs';

export function checkQualifiedRun({ run, jobs, sourceSha, branch, waiver }) {
  if (!isReleasePushRun(run, sourceSha, SOURCE_REPO)) refuse(`run ${run.id} is not an owner release tests.yml push run`);
  const waived = waiver?.actor === 'rollacode' && waiver.run_id === String(run.id);
  if (run.status !== 'completed' || (run.conclusion !== 'success' && !(waived && run.conclusion === 'failure'))) refuse(`run ${run.id} is ${run.status}/${run.conclusion}`);
  if (run.head_sha !== sourceSha) refuse(`run ${run.id} built ${run.head_sha}, not ${sourceSha}`);
  if (run.head_branch !== branch) refuse(`run ${run.id} ran on ${run.head_branch}, not ${branch}`);
  for (const job of jobs) {
    if (!['success', 'skipped'].includes(job.conclusion) && !(waived && job.conclusion === 'failure' &&
        (waiver.jobs.includes(job.name) || waiver.derived_jobs.some(row => row.name === job.name && row.conclusion === job.conclusion)))) {
      refuse(`${job.name} is ${job.conclusion}`);
    }
  }
  for (const required of REQUIRED_JOBS) {
    const prefix = 'Release packaging / ' + required;
    const matching = jobs.filter(job => job.name.startsWith(prefix));
    if (!matching.length) refuse(`run ${run.id} has no ${required.replace(/ \/ $/, '')} job`);
    const unfinished = matching.find(job => job.conclusion !== 'success');
    if (unfinished) refuse(`${unfinished.name} is ${unfinished.conclusion}`);
  }
}

async function gh(args, env) {
  const { stdout, stderr } = await exec('gh', args, { env, encoding: 'utf8', timeout: 30 * 60_000, maxBuffer: 8 * 1024 * 1024 });
  if (stderr) process.stderr.write(stderr);
  return stdout;
}

export async function publishPlatform({ tag, directory, receipt, repository, env, run = gh }) {
  const plan = promotionPlan(receipt);
  const names = repository === MIRROR_REPO ? plan.mirror : plan.source;
  const rows = new Map(receipt.files.map(file => [file.name, file]));
  await run(['release', 'upload', tag, ...names.map(name => path.join(directory, name)), '--clobber', '--repo', repository], env);
  // The releases/tags endpoint hides drafts; `gh release view` resolves the draft by tag.
  const release = JSON.parse(await run(['release', 'view', tag, '--repo', repository, '--json', 'databaseId'], env));
  if (!Number.isInteger(release.databaseId)) refuse(`${repository} ${tag} draft release id is missing`);
  const assets = (await run(['api', '--paginate', '--jq', '.[] | tojson',
    `repos/${repository}/releases/${release.databaseId}/assets?per_page=100`], env))
    .split('\n').filter(Boolean).map(line => JSON.parse(line));
  verifyAssetDigests(names.map(name => rows.get(name)), assets, repository);
  return names;
}

export async function publishConcurrently(tasks, publish, limit = PUBLICATION_CONCURRENCY) {
  if (!Number.isInteger(limit) || limit < 1) refuse('invalid publication concurrency');
  let next = 0;
  let failure;
  const workers = Array.from({ length: Math.min(limit, tasks.length) }, async () => {
    while (!failure && next < tasks.length) {
      const task = tasks[next++];
      try { await publish(task); } catch (error) { failure ??= error; }
    }
  });
  await Promise.all(workers);
  if (failure) throw failure;
}

export async function promoteQualifiedDrafts({ tag, sourceSha, runId, work, repositories,
  requireDraft = false,
  fetchEvidence = fetchAndAggregate, publish = publishPlatform, run = gh, drafts,
  readReceipt = async directory => JSON.parse(await readFile(path.join(directory, 'dry-run-evidence.json'), 'utf8')) }) {
  for (const [repository, token] of repositories) {
    if (!token) refuse(`no token for ${repository}`);
  }
  const { readDraft, admitDraft, qualificationTag, prepareDraft, sealDraft } = drafts ?? await import('./release-qualification-draft.mjs');
  const missing = [];
  for (const [repository, token] of repositories) {
    const env = { ...process.env, GH_TOKEN: token };
    const draft = await readDraft(qualificationTag(tag, runId), repository, env) ?? await readDraft(tag, repository, env);
    if (draft) {
      admitDraft(draft, tag, sourceSha, runId, repository);
      if (!draft.body.includes('<!-- qualification-complete -->')) missing.push([repository, token]);
      if (draft.tagName !== tag) await run(['api', '--method', 'PATCH', `repos/${repository}/releases/${draft.databaseId}`, '-f', `tag_name=${tag}`], env);
    }
    else missing.push([repository, token]);
  }
  if (!missing.length) {
    console.log(`Admitted qualification drafts for ${sourceSha} from run ${runId}; no asset transfers`);
    return;
  }
  if (requireDraft) refuse('owner test waiver requires complete existing qualification drafts');
  const input = path.join(work, 'input');
  await fetchEvidence({ tag, sourceSha, runId, inputDirectory: input, outputDirectory: path.join(work, 'output') });
  for (const [repository, token] of missing) {
    const env = { ...process.env, GH_TOKEN: token };
    let draft = await readDraft(tag, repository, env);
    if (!draft) {
      await prepareDraft({ tag, sourceSha, runId, repository, env });
      draft = await readDraft(qualificationTag(tag, runId), repository, env);
    }
    await run(['api', '--method', 'PATCH', `repos/${repository}/releases/${draft.databaseId}`, '-f', `tag_name=${tag}`], env);
  }
  const tasks = [];
  for (const platform of PLATFORMS) {
    const directory = path.join(input, platform);
    const receipt = await readReceipt(directory);
    for (const [repository, token] of missing) {
      tasks.push({ tag, platform, directory, receipt, repository, env: { ...process.env, GH_TOKEN: token } });
    }
  }
  const started = performance.now();
  await publishConcurrently(tasks, async task => {
    const names = await publish(task);
    console.log(`${task.platform}: ${names.length} retained file(s) digest-verified on ${task.repository} draft ${tag}`);
  });
  for (const [repository, token] of missing) await sealDraft({ tag, sourceSha, runId, repository, env: { ...process.env, GH_TOKEN: token } });
  console.log(`Digest-verified publication completed in ${((performance.now() - started) / 1000).toFixed(1)}s (concurrency ${PUBLICATION_CONCURRENCY})`);
}

async function main([verb, ...args]) {
  if (verb === 'check') {
    const [runId, sourceSha, branch] = args;
    if (!/^[1-9]\d*$/.test(runId ?? '')) refuse('invalid dry-run run ID');
    // Runner gh builds predate --slurp; --jq emits one job object per line across pages.
    const run = JSON.parse(execFileSync('gh', ['api', `repos/${SOURCE_REPO}/actions/runs/${runId}`], { encoding: 'utf8' }));
    const jobs = execFileSync('gh', ['api', '--paginate', '--jq', '.jobs[] | tojson', `repos/${SOURCE_REPO}/actions/runs/${runId}/jobs?per_page=100`],
      { encoding: 'utf8' }).split('\n').filter(Boolean).map(line => JSON.parse(line));
    const receipt = requireQualification(sourceSha, { runId });
    checkQualifiedRun({ run, jobs, sourceSha, branch, waiver: receipt.test_waiver });
    if (run.path === '.github/workflows/tests.yml' && String(receipt.run_id) !== String(run.id)) refuse('qualification belongs to a different release push run');
    console.log(`run ${runId} qualifies ${sourceSha} on ${branch}`);
  } else if (verb === 'publish') {
    const [tag, sourceSha, runId, work] = args;
    const receipt = requireQualification(sourceSha, { runId });
    const repositories = [[SOURCE_REPO, process.env.GH_TOKEN],
      ...(process.env.PUBLISH_MIRROR === 'true' ? [[MIRROR_REPO, process.env.MIRROR_TOKEN]] : [])];
    await promoteQualifiedDrafts({ tag, sourceSha, runId, work, repositories, requireDraft: Boolean(receipt.test_waiver) });
  } else refuse('unknown operation');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
