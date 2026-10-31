import { sourceRepository } from './source-repository.mjs';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { verifyReceipts, readReceipts } from './ci-rust-receipts.mjs';
import { readQualificationArchive } from './ci-qualification-archive.mjs';

export const QUALIFICATION_JOBS = [
  'Repository guards', 'CI result', 'Fast checks / script tests and rustfmt',
  'Fast checks / ts static checks', 'Fast checks / frontend producer',
  'Fast checks / ts generated types', 'Fast checks / ts unit', 'Fast checks / ts vitest',
  'Fast checks / plugin bundles + tests', 'Rust tests / rust tests (full suite)',
  'Rust tests / rust platform (macOS)', 'Rust tests / rust platform (Windows)',
  'Rust tests / rust embeddings', 'Rust tests / rust test receipts', 'qualification receipt',
];

export function latestQualificationJobs(jobs, run, sha) {
  const latest = new Map();
  for (const job of jobs) {
    if (String(job.run_id) !== String(run.id) || job.head_sha !== sha
      || !Number.isSafeInteger(job.run_attempt) || job.run_attempt < 1 || job.run_attempt > run.run_attempt
      || !Number.isSafeInteger(job.id) || job.id < 1) throw new Error('Foreign qualification job provenance');
    const previous = latest.get(job.name);
    if (!previous || job.run_attempt > previous.run_attempt) latest.set(job.name, job);
    else if (job.run_attempt === previous.run_attempt && job.id !== previous.id) throw new Error('Ambiguous qualification job');
  }
  return [...latest.values()];
}

export function qualificationAttemptNumbers(run) {
  if (!Number.isSafeInteger(run.run_attempt) || run.run_attempt < 1 || run.run_attempt > 20) {
    throw new Error('Qualification attempt history exceeds bound or is invalid');
  }
  return Array.from({ length: run.run_attempt }, (_, index) => index + 1);
}

function executionTime(value) {
  if (typeof value !== 'string' || !Number.isFinite(Date.parse(value))) throw new Error('Missing qualification execution timestamp');
  return Date.parse(value);
}

function executionTrace(job) {
  const start = executionTime(job.started_at), end = executionTime(job.completed_at);
  if (job.status !== 'completed' || job.conclusion !== 'success' || start > end
    || typeof job.name !== 'string' || !job.name || !Number.isSafeInteger(job.runner_id) || job.runner_id < 1
    || typeof job.runner_name !== 'string' || !job.runner_name || !Array.isArray(job.labels) || !job.labels.length
    || job.labels.some(label => typeof label !== 'string' || !label)
    || !Array.isArray(job.steps) || !job.steps.length) throw new Error('Incomplete qualification execution trace');
  let previousNumber = 0;
  const steps = job.steps.map(step => {
    const stepStart = executionTime(step.started_at), stepEnd = executionTime(step.completed_at);
    if (typeof step.name !== 'string' || !step.name || !Number.isSafeInteger(step.number) || step.number <= previousNumber
      || step.status !== 'completed' || typeof step.conclusion !== 'string' || !step.conclusion
      || stepStart < start || stepEnd > end || stepStart > stepEnd) throw new Error('Incomplete ordered qualification steps');
    previousNumber = step.number;
    return [step.name, step.number, step.status, step.conclusion, step.started_at, step.completed_at];
  });
  return JSON.stringify([job.name, job.runner_id, job.runner_name, job.labels,
    job.started_at, job.completed_at, steps]);
}

export function resolveQualificationOrigins(required, jobs, run, attempts, sha, repository) {
  const numbers = qualificationAttemptNumbers(run);
  if (!Array.isArray(attempts) || attempts.length !== numbers.length
    || !Number.isSafeInteger(run.workflow_id) || run.workflow_id < 1) throw new Error('Incomplete qualification attempt history');
  if (!Array.isArray(jobs) || jobs.length > 2000) throw new Error('Qualification job inventory exceeds bound');
  const starts = new Map();
  let previousStart = -Infinity;
  for (const number of numbers) {
    const matching = attempts.filter(attempt => attempt.run_attempt === number);
    if (matching.length !== 1) throw new Error('Ambiguous qualification attempt metadata');
    const attempt = matching[0], start = executionTime(attempt.run_started_at);
    if (String(attempt.id) !== String(run.id) || attempt.head_sha !== sha || attempt.path !== run.path
      || attempt.repository?.full_name !== repository || attempt.head_repository?.full_name !== repository
      || attempt.event !== run.event || attempt.head_branch !== run.head_branch || attempt.workflow_id !== run.workflow_id
      || attempt.status !== 'completed' || typeof attempt.conclusion !== 'string' || !attempt.conclusion
      || start <= previousStart) throw new Error('Foreign or conflicting qualification attempt metadata');
    starts.set(number, start);
    previousStart = start;
  }
  if (executionTime(run.run_started_at) !== starts.get(run.run_attempt)) throw new Error('Conflicting current qualification attempt');
  const identities = new Set(), ids = new Set();
  for (const job of jobs) {
    const identity = JSON.stringify([job.run_attempt, job.name]);
    if (String(job.run_id) !== String(run.id) || job.head_sha !== sha || !starts.has(job.run_attempt)
      || !Number.isSafeInteger(job.id) || job.id < 1 || identities.has(identity) || ids.has(job.id)) {
      throw new Error('Foreign or ambiguous qualification execution inventory');
    }
    identities.add(identity);
    ids.add(job.id);
  }
  const origins = new Map(), traces = new Map();
  const trace = job => {
    if (!traces.has(job.id)) traces.set(job.id, executionTrace(job));
    return traces.get(job.id);
  };
  const resolve = job => {
    if (origins.has(job.id)) return origins.get(job.id);
    const fingerprint = trace(job), attemptStart = starts.get(job.run_attempt);
    let origin = job;
    if (executionTime(job.started_at) < attemptStart) {
      if (executionTime(job.completed_at) >= attemptStart) throw new Error('Conflicting retained qualification execution time');
      const candidates = jobs.filter(candidate => candidate.name === job.name && candidate.run_attempt < job.run_attempt
        && candidate.status === 'completed' && candidate.conclusion === 'success' && trace(candidate) === fingerprint);
      const authoritative = new Map(candidates.map(candidate => {
        const original = resolve(candidate);
        return [original.id, original];
      }));
      if (authoritative.size !== 1) throw new Error('Missing or ambiguous retained qualification execution origin');
      origin = [...authoritative.values()][0];
    }
    origins.set(job.id, origin);
    return origin;
  };
  for (const job of required) resolve(job);
  return origins;
}

export function verifyQualification(receipt, run, jobs, sha, repository = sourceRepository(), attempts = []) {
  if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error('Invalid qualification SHA');
  if (run.path !== '.github/workflows/tests.yml' || run.status !== 'completed' || run.conclusion !== 'success') throw new Error('Qualification run is not green tests.yml');
  const trustedBranch = ['develop', 'rename/domios', 'master'].includes(run.head_branch) || /^release\/\d+\.\d+\.\d+$/.test(run.head_branch);
  const ownHead = run.head_repository?.full_name === repository && run.repository?.full_name === repository;
  if (!(['push', 'pull_request'].includes(run.event) && trustedBranch && ownHead && run.head_sha === sha)) throw new Error('Untrusted qualification source');
  if (receipt.version !== 2 || receipt.sha !== sha || String(receipt.run_id) !== String(run.id)
    || receipt.repository !== repository || receipt.workflow !== run.path
    || !Number.isSafeInteger(receipt.run_attempt) || receipt.run_attempt < 1 || receipt.run_attempt > run.run_attempt) throw new Error('Foreign or stale qualification receipt');
  if (!Number.isSafeInteger(run.run_attempt) || run.run_attempt < 1) throw new Error('Invalid qualification run attempt');
  const effective = latestQualificationJobs(jobs, run, sha);
  const required = new Set();
  for (const stage of ['fast', 'rust', 'warnings']) if (receipt.results?.[stage] !== 'success') throw new Error(`Missing green ${stage} receipt`);
  for (const prefix of QUALIFICATION_JOBS) {
    const matching = effective.filter(job => job.name === prefix || job.name.startsWith(prefix + ' ('));
    if (!matching.length || matching.some(job => job.status !== 'completed' || job.conclusion !== 'success')) throw new Error(`Missing green qualification job: ${prefix}`);
    for (const job of matching) required.add(job);
  }
  const origins = resolveQualificationOrigins(required, jobs, run, attempts, sha, repository);
  const producer = effective.find(job => job.name === 'qualification receipt');
  if (origins.get(producer.id).run_attempt !== receipt.run_attempt) throw new Error('Stale qualification receipt producer');
  const rustJobs = { full: 'Rust tests / rust tests (full suite)', embeddings: 'Rust tests / rust embeddings',
    macOS: 'Rust tests / rust platform (macOS)', Windows: 'Rust tests / rust platform (Windows)' };
  for (const row of receipt.rust) {
    const name = rustJobs[row.mode === 'platform' ? row.os : row.mode];
    const producers = effective.filter(job => job.name === name || job.name.startsWith(name + ' ('));
    const original = producers.length === 1 ? origins.get(producers[0].id) : undefined;
    if (!name || !original || row.run_attempt !== original.run_attempt
      || row.run_attempt > receipt.run_attempt || String(row.run_id) !== String(run.id)
      || row.repository !== repository || !row.runner || row.runner !== original.runner_name
      || row.producer !== (row.mode === 'full' ? 'rust-tests' : row.mode === 'platform' ? 'rust-platform' : 'rust-embeddings')) throw new Error('Foreign or stale Rust receipt provenance');
  }
  if (receipt.rust.some(row => row.sha !== sha)) throw new Error('Foreign Rust qualification SHA');
  verifyReceipts(receipt.rust);
  return { ...receipt, verified_run_attempt: run.run_attempt, verified_jobs: effective.map(job => ({
    id: job.id, name: job.name, run_id: job.run_id, run_attempt: job.run_attempt,
    head_sha: job.head_sha, conclusion: job.conclusion,
    ...(origins.has(job.id) ? { execution_id: origins.get(job.id).id, execution_run_attempt: origins.get(job.id).run_attempt } : {}),
  })) };
}

export function retainQualificationEvidence(receipt) {
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY,
    '\nQualification provenance\n\n```json\n' + JSON.stringify(receipt, null, 2) + '\n```\n');
  return receipt;
}

export function recordQualification(sha, rust, results, { repository, run_id, run_attempt }) {
  if (!/^[a-f0-9]{40}$/.test(sha) || !/^[1-9]\d*$/.test(String(run_id))
    || !Number.isSafeInteger(run_attempt) || run_attempt < 1 || !/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid qualification producer');
  verifyReceipts(rust);
  for (const row of rust) {
    if (row.sha !== sha || row.repository !== repository || String(row.run_id) !== String(run_id)
      || !Number.isSafeInteger(row.run_attempt) || row.run_attempt < 1 || row.run_attempt > run_attempt) throw new Error('Foreign Rust qualification producer');
  }
  for (const stage of ['fast', 'rust', 'warnings']) {
    if (results?.[stage] !== 'success') throw new Error('Qualification stages are not green');
  }
  return { version: 2, sha, repository, workflow: '.github/workflows/tests.yml', run_id, run_attempt, results, rust };
}

const gh = args => execFileSync('gh', args, { encoding: 'utf8', timeout: 120_000, maxBuffer: 16 * 1024 * 1024 });
const api = route => JSON.parse(gh(['api', route]));

export function selectQualificationArtifact(artifacts, sha, now = Date.now()) {
  const name = `qualification-${sha}`;
  const artifact = artifacts.filter(row => row.name === name && !row.expired
    && Date.parse(row.expires_at) > now && row.workflow_run?.head_sha === sha)
    .sort((a, b) => b.id - a.id)[0];
  if (!artifact) throw new Error(`No unexpired qualification receipt for ${sha}`);
  return artifact;
}

export function qualificationJobs(run, repository, request) {
  const jobs = [];
  for (let page = 1; page <= 20; page++) {
    const batch = request(`repos/${repository}/actions/runs/${run.id}/jobs?filter=all&per_page=100&page=${page}`);
    jobs.push(...batch.jobs);
    if (jobs.length >= batch.total_count) return jobs;
    if (!batch.jobs.length) break;
  }
  throw new Error('Qualification job inventory exceeds bound or is incomplete');
}

export function requireQualification(sha, { repository = sourceRepository(), request = api,
  download = args => execFileSync('gh', args, { timeout: 120_000, maxBuffer: 16 * 1024 * 1024 }),
  readArchive = readQualificationArchive, retain = retainQualificationEvidence } = {}) {
  if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error('Invalid qualification SHA');
  const name = `qualification-${sha}`;
  const artifacts = request(`repos/${repository}/actions/artifacts?name=${name}&per_page=100`).artifacts;
  const artifact = selectQualificationArtifact(artifacts, sha);
  const run = request(`repos/${repository}/actions/runs/${artifact.workflow_run.id}`);
  if (String(run.id) !== String(artifact.workflow_run.id)) throw new Error('Foreign qualification artifact run');
  const jobs = qualificationJobs(run, repository, request);
  const attempts = qualificationAttemptNumbers(run).map(number => request(`repos/${repository}/actions/runs/${run.id}/attempts/${number}`));
  const archive = download(['api', `repos/${repository}/actions/artifacts/${artifact.id}/zip`]);
  return retain({ ...verifyQualification(readArchive(archive), run, jobs, sha, repository, attempts),
    verification_artifact: { id: artifact.id, expires_at: artifact.expires_at } });
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [verb, sha, directory] = process.argv.slice(2);
  if (verb === 'check') {
    const receipt = requireQualification(sha);
    console.log(`Qualified ${sha} in tests run ${receipt.run_id}, attempt ${receipt.run_attempt}`);
  } else if (verb === 'record') {
    const rust = readReceipts(directory);
    const results = JSON.parse(process.env.QUALIFICATION_RESULTS);
    const receipt = recordQualification(sha, rust, results, { repository: sourceRepository(),
      run_id: process.env.GITHUB_RUN_ID, run_attempt: Number(process.env.GITHUB_RUN_ATTEMPT) });
    fs.writeFileSync('qualification.json', JSON.stringify(receipt, null, 2) + '\n');
  } else throw new Error('Usage: ci-qualification.mjs check SHA | record SHA RECEIPTS');
}
