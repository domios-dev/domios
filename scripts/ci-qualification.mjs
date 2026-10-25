import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { verifyReceipts } from './ci-rust-receipts.mjs';

export const QUALIFICATION_JOBS = [
  'Fast checks / guards', 'Fast checks / clippy', 'Fast checks / cli tests',
  'Fast checks / ts static checks', 'Fast checks / frontend producer', 'Fast checks / Windows path identity',
  'Fast checks / ts generated types', 'Fast checks / ts unit', 'Fast checks / ts vitest',
  'Fast checks / plugin bundles + tests', 'Rust tests / rust tests (full suite)',
  'Rust tests / rust platform (macOS)', 'Rust tests / rust platform (Windows)',
  'Rust tests / rust embeddings', 'Rust tests / rust test receipts', 'qualification receipt',
];

export function verifyQualification(receipt, run, jobs, sha) {
  if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error('Invalid qualification SHA');
  if (run.path !== '.github/workflows/tests.yml' || run.status !== 'completed' || run.conclusion !== 'success') throw new Error('Qualification run is not green tests.yml');
  if (!(run.event === 'push' && ['develop', 'rename/domios', 'master'].includes(run.head_branch) && run.head_sha === sha || run.event === 'schedule')) throw new Error('Untrusted qualification source');
  if (receipt.version !== 1 || receipt.sha !== sha || String(receipt.run_id) !== String(run.id) || receipt.run_attempt !== run.run_attempt) throw new Error('Foreign or stale qualification receipt');
  for (const stage of ['fast', 'rust', 'warnings']) if (receipt.results?.[stage] !== 'success') throw new Error(`Missing green ${stage} receipt`);
  for (const prefix of QUALIFICATION_JOBS) {
    const matching = jobs.filter(job => job.name.startsWith(prefix));
    if (!matching.length || matching.some(job => job.conclusion !== 'success')) throw new Error(`Missing green qualification job: ${prefix}`);
  }
  if (receipt.rust.some(row => row.sha !== sha)) throw new Error('Foreign Rust qualification SHA');
  verifyReceipts(receipt.rust);
  return receipt;
}

const gh = args => execFileSync('gh', args, { encoding: 'utf8', timeout: 120_000, maxBuffer: 16 * 1024 * 1024 });
const api = route => JSON.parse(gh(['api', route]));

export function requireQualification(sha, { repository = process.env.GITHUB_REPOSITORY || 'rollacode/codeterm', request = api, download = gh } = {}) {
  if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error('Invalid qualification SHA');
  const name = `qualification-${sha}`;
  const artifacts = request(`repos/${repository}/actions/artifacts?name=${name}&per_page=100`).artifacts;
  const artifact = artifacts.filter(row => row.name === name && !row.expired).sort((a, b) => b.id - a.id)[0];
  if (!artifact) throw new Error(`No qualification receipt for ${sha}; qualify this exact SHA on dev first`);
  const run = request(`repos/${repository}/actions/runs/${artifact.workflow_run.id}`);
  const jobs = [];
  for (let page = 1; ; page++) {
    if (page > 20) throw new Error('Qualification job inventory exceeds bound');
    const batch = request(`repos/${repository}/actions/runs/${run.id}/attempts/${run.run_attempt}/jobs?per_page=100&page=${page}`);
    jobs.push(...batch.jobs);
    if (jobs.length >= batch.total_count) break;
  }
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'domios-qualification-'));
  try {
    download(['run', 'download', String(run.id), '--repo', repository, '--name', name, '--dir', directory]);
    return verifyQualification(JSON.parse(fs.readFileSync(path.join(directory, 'qualification.json'), 'utf8')), run, jobs, sha);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [verb, sha, directory] = process.argv.slice(2);
  if (verb === 'check') {
    const receipt = requireQualification(sha);
    console.log(`Qualified ${sha} in tests run ${receipt.run_id}, attempt ${receipt.run_attempt}`);
  } else if (verb === 'record') {
    const rust = fs.readdirSync(directory).filter(name => name.endsWith('.json')).map(name => JSON.parse(fs.readFileSync(path.join(directory, name), 'utf8')));
    verifyReceipts(rust);
    if (rust.some(row => row.sha !== sha)) throw new Error('Foreign Rust source');
    const results = JSON.parse(process.env.QUALIFICATION_RESULTS);
    if (Object.values(results).some(value => value !== 'success')) throw new Error('Qualification stages are not green');
    fs.writeFileSync('qualification.json', JSON.stringify({ version: 1, sha, run_id: process.env.GITHUB_RUN_ID, run_attempt: Number(process.env.GITHUB_RUN_ATTEMPT), results, rust }, null, 2) + '\n');
  } else throw new Error('Usage: ci-qualification.mjs check SHA | record SHA RECEIPTS');
}
