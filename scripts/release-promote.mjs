#!/usr/bin/env node
import { sourceRepository } from './source-repository.mjs';
// Caller: CI release.yml (prepare, promote-evidence); skill github-release
// Promotes a qualified release.yml dry run: its retained bytes become the release, nothing is rebuilt.
//
//   release-promote.mjs check <run-id> <source-sha> <release-branch>
//   release-promote.mjs publish <tag> <source-sha> <run-id> <work-dir>
import { requireQualification } from './ci-qualification.mjs';
import { execFile, execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { fetchAndAggregate, PLATFORMS } from './release-dry-run-manifest.mjs';

const SOURCE_REPO = sourceRepository();
// 1.19 transition mirror: PUBLISH_MIRROR also writes this repository; delete after 1.19 ships.
const MIRROR_REPO = 'rollacode/codeterm-releases';
const exec = promisify(execFile);
export const PUBLICATION_CONCURRENCY = 3;

// Each name prefixes a release.yml job (reusable callers add " / <job>") that must have succeeded.
export const REQUIRED_JOBS = [
  'prepare release',
  'macOS arm64 / ', 'macOS x64 / ', 'Linux arm64 / ', 'Linux x64 / ', 'Windows x64 / ', 'Windows arm64 / ',
  'local dry-run manifest verification / ',
];

function refuse(message) {
  throw new Error('promotion refused: ' + message);
}

export function isReleasePushRun(run, sha) {
  return run.event === 'pull_request' && run.head_sha === sha && /^release\/\d+\.\d+\.\d+$/.test(run.head_branch) &&
    ['rollacode','domios-dev'].includes(run.repository?.owner?.login) &&
    run.head_repository?.full_name === run.repository?.full_name &&
    run.actor?.login === 'rollacode' && run.triggering_actor?.login === 'rollacode';
}

export function checkQualifiedRun({ run, jobs, sourceSha, branch }) {
  const pushRun = run.path === '.github/workflows/tests.yml' && isReleasePushRun(run, sourceSha);
  if (run.path !== '.github/workflows/release.yml' && !pushRun) refuse(`run ${run.id} is not a release.yml run`);
  if (run.status !== 'completed' || run.conclusion !== 'success') refuse(`run ${run.id} is ${run.status}/${run.conclusion}`);
  if (run.head_sha !== sourceSha) refuse(`run ${run.id} built ${run.head_sha}, not ${sourceSha}`);
  if (run.head_branch !== branch) refuse(`run ${run.id} ran on ${run.head_branch}, not ${branch}`);
  for (const job of jobs) {
    if (!['success', 'skipped'].includes(job.conclusion)) refuse(`${job.name} is ${job.conclusion}`);
  }
  for (const required of REQUIRED_JOBS) {
    const prefix = pushRun ? 'Release packaging / ' + required : required;
    const matching = jobs.filter(job => job.name.startsWith(prefix));
    if (!matching.length) refuse(`run ${run.id} has no ${required.replace(/ \/ $/, '')} job`);
    const unfinished = matching.find(job => job.conclusion !== 'success');
    if (unfinished) refuse(`${unfinished.name} is ${unfinished.conclusion}`);
  }
}

// Mirrors release-publish-platform.mjs: the source draft also takes the updater fragments, the public mirror does not.
export function promotionPlan(receipt) {
  const retained = receipt.files.map(file => file.name);
  return {
    source: retained,
    mirror: retained.filter(name => !/^updater-.*\.json$/.test(name)),
  };
}

export function verifyAssetDigests(files, assets, repository) {
  for (const file of files) {
    const matching = assets.filter(asset => asset.name === file.name);
    if (matching.length !== 1) refuse(`${repository} expected one uploaded asset ${file.name}`);
    const asset = matching[0];
    if (!/^sha256:[a-f0-9]{64}$/.test(asset.digest ?? '')) {
      refuse(`${repository} ${file.name} has no valid sha256 API digest`);
    }
    if (!/^[a-f0-9]{64}$/.test(file.sha256 ?? '') || asset.digest !== `sha256:${file.sha256}` ||
        asset.size !== file.size || asset.state !== 'uploaded') {
      refuse(`${repository} ${file.name} differs from its retained receipt`);
    }
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

async function main([verb, ...args]) {
  if (verb === 'check') {
    const [runId, sourceSha, branch] = args;
    if (!/^[1-9]\d*$/.test(runId ?? '')) refuse('invalid dry-run run ID');
    // Runner gh builds predate --slurp; --jq emits one job object per line across pages.
    const run = JSON.parse(execFileSync('gh', ['api', `repos/${SOURCE_REPO}/actions/runs/${runId}`], { encoding: 'utf8' }));
    const jobs = execFileSync('gh', ['api', '--paginate', '--jq', '.jobs[] | tojson', `repos/${SOURCE_REPO}/actions/runs/${runId}/jobs?per_page=100`],
      { encoding: 'utf8' }).split('\n').filter(Boolean).map(line => JSON.parse(line));
    checkQualifiedRun({ run, jobs, sourceSha, branch });
    const receipt = requireQualification(sourceSha);
    if (run.path === '.github/workflows/tests.yml' && String(receipt.run_id) !== String(run.id)) refuse('qualification belongs to a different release push run');
    console.log(`run ${runId} qualifies ${sourceSha} on ${branch}`);
  } else if (verb === 'publish') {
    const [tag, sourceSha, runId, work] = args;
    requireQualification(sourceSha);
    const input = path.join(work, 'input');
    await fetchAndAggregate({ tag, sourceSha, runId, inputDirectory: input, outputDirectory: path.join(work, 'output') });
    const repositories = [[SOURCE_REPO, process.env.GH_TOKEN],
      ...(process.env.PUBLISH_MIRROR === 'true' ? [[MIRROR_REPO, process.env.MIRROR_TOKEN]] : [])];
    for (const [repository, token] of repositories) {
      if (!token) refuse(`no token for ${repository}`);
    }
    const tasks = [];
    for (const platform of PLATFORMS) {
      const directory = path.join(input, platform);
      const receipt = JSON.parse(await readFile(path.join(directory, 'dry-run-evidence.json'), 'utf8'));
      for (const [repository, token] of repositories) {
        tasks.push({ tag, platform, directory, receipt, repository, env: { ...process.env, GH_TOKEN: token } });
      }
    }
    const started = performance.now();
    await publishConcurrently(tasks, async task => {
      const names = await publishPlatform(task);
      console.log(`${task.platform}: ${names.length} retained file(s) digest-verified on ${task.repository} draft ${tag}`);
    });
    console.log(`Digest-verified publication completed in ${((performance.now() - started) / 1000).toFixed(1)}s (concurrency ${PUBLICATION_CONCURRENCY})`);
  } else refuse('unknown operation');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
