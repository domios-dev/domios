#!/usr/bin/env node
import { sourceRepository } from './source-repository.mjs';
// Caller: CI release.yml (prepare, promote-evidence); skill github-release
// Promotes a qualified release.yml dry run: its retained bytes become the release, nothing is rebuilt.
//
//   release-promote.mjs check <run-id> <source-sha> <release-branch>
//   release-promote.mjs publish <tag> <source-sha> <run-id> <work-dir>
import { requireQualification } from './ci-qualification.mjs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { headlessAssetNames } from './headless-release-package.mjs';
import { publishHeadlessAssets } from './release-headless-assets.mjs';
import { fetchAndAggregate, PLATFORMS } from './release-dry-run-manifest.mjs';

const SOURCE_REPO = sourceRepository();
const MIRROR_REPO = 'rollacode/codeterm-releases';

// Each name prefixes a release.yml job (reusable callers add " / <job>") that must have succeeded.
export const REQUIRED_JOBS = [
  'prepare release',
  'macOS arm64 / ', 'macOS x64 / ', 'Linux arm64 / ', 'Linux x64 / ', 'Windows x64 / ', 'Windows arm64 / ',
  'local dry-run manifest verification / ',
];

function refuse(message) {
  throw new Error('promotion refused: ' + message);
}

export function checkQualifiedRun({ run, jobs, sourceSha, branch }) {
  if (run.path !== '.github/workflows/release.yml') refuse(`run ${run.id} is not a release.yml run`);
  if (run.status !== 'completed' || run.conclusion !== 'success') refuse(`run ${run.id} is ${run.status}/${run.conclusion}`);
  if (run.head_sha !== sourceSha) refuse(`run ${run.id} built ${run.head_sha}, not ${sourceSha}`);
  if (run.head_branch !== branch) refuse(`run ${run.id} ran on ${run.head_branch}, not ${branch}`);
  for (const job of jobs) {
    if (!['success', 'skipped'].includes(job.conclusion)) refuse(`${job.name} is ${job.conclusion}`);
  }
  for (const required of REQUIRED_JOBS) {
    const matching = jobs.filter(job => job.name.startsWith(required));
    if (!matching.length) refuse(`run ${run.id} has no ${required.replace(/ \/ $/, '')} job`);
    const unfinished = matching.find(job => job.conclusion !== 'success');
    if (unfinished) refuse(`${unfinished.name} is ${unfinished.conclusion}`);
  }
}

// Mirrors release-publish-platform.mjs: the source draft also takes the updater fragments, the public mirror does not.
export function promotionPlan(tag, platform, receipt) {
  const names = headlessAssetNames(tag, platform);
  const headless = [names.archive, names.checksum, names.manifest];
  const retained = receipt.files.map(file => file.name).filter(name => !headless.includes(name));
  return {
    source: retained,
    mirror: retained.filter(name => !/^updater-.*\.json$/.test(name)),
  };
}

async function sha256(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

function gh(args, env) {
  execFileSync('gh', args, { env, stdio: 'inherit', timeout: 30 * 60_000 });
}

export async function publishPlatform({ tag, sourceSha, platform, directory, receipt, repository, env, run = gh,
  publishHeadless = publishHeadlessAssets }) {
  const plan = promotionPlan(tag, platform, receipt);
  const names = repository === MIRROR_REPO ? plan.mirror : plan.source;
  const rows = new Map(receipt.files.map(file => [file.name, file]));
  run(['release', 'upload', tag, ...names.map(name => path.join(directory, name)), '--clobber', '--repo', repository], env);
  const scratch = await mkdtemp(path.join(os.tmpdir(), 'domios-promote-readback-'));
  try {
    for (const name of names) {
      run(['release', 'download', tag, '--pattern', name, '--dir', scratch, '--clobber', '--repo', repository], env);
      if (await sha256(path.join(scratch, name)) !== rows.get(name).sha256) refuse(`${repository} ${name} differs from its ${platform} receipt`);
    }
  } finally { await rm(scratch, { recursive: true, force: true }); }
  await publishHeadless({ tag, platform, directory, sourceSha, repository, env });
  return names;
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
    requireQualification(sourceSha);
    console.log(`run ${runId} qualifies ${sourceSha} on ${branch}`);
  } else if (verb === 'publish') {
    const [tag, sourceSha, runId, work] = args;
    requireQualification(sourceSha);
    const input = path.join(work, 'input');
    await fetchAndAggregate({ tag, sourceSha, runId, inputDirectory: input, outputDirectory: path.join(work, 'output') });
    const repositories = [[SOURCE_REPO, process.env.GH_TOKEN],
      ...(process.env.PUBLISH_MIRROR === 'true' ? [[MIRROR_REPO, process.env.MIRROR_TOKEN]] : [])];
    for (const platform of PLATFORMS) {
      const directory = path.join(input, platform);
      const receipt = JSON.parse(await readFile(path.join(directory, 'dry-run-evidence.json'), 'utf8'));
      for (const [repository, token] of repositories) {
        if (!token) refuse(`no token for ${repository}`);
        const names = await publishPlatform({ tag, sourceSha, platform, directory, receipt, repository, env: { ...process.env, GH_TOKEN: token } });
        console.log(`${platform}: ${names.length} retained file(s) and the headless assets published to ${repository} draft ${tag}`);
      }
    }
  } else refuse('unknown operation');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
