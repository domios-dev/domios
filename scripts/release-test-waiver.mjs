import { releaseTag } from './release-version.mjs';
import { isReleasePushRun, latestQualificationJobs, QUALIFICATION_JOBS, resolveQualificationOrigins } from './ci-qualification.mjs';
import { REQUIRED_JOBS, verifyAssetDigests } from './release-promote.mjs';
import { PLATFORMS } from './release-dry-run-manifest.mjs';

const TEST_STEPS = new Map([
  ['Fast checks / ts unit', ['Run TS unit tests']],
  ['Fast checks / ts vitest', ['Run vitest-owned TS tests']],
  ['Fast checks / plugin bundles + tests', ['Run plugin tests']],
  ['Fast checks / script tests and rustfmt', ['Plugin release contracts', 'Offline core journey contracts']],
  ['Rust tests / rust tests (full suite)', ['Run full Rust tests (Unix)', 'Run full Rust tests (Windows)']],
  ['Rust tests / rust platform (macOS)', ['Run platform Rust tests (Unix)']],
  ['Rust tests / rust platform (Windows)', ['Run platform Rust tests (Windows)']],
  ['Rust tests / rust embeddings', ['Run embeddings Rust tests (Unix)', 'Run embeddings Rust tests (Windows)']],
]);
const REPORT_STEPS = new Map([
  ['CI result', 'Require the complete event graph'],
  ['Rust tests / rust test receipts', 'Require every Rust test job'],
  ['rust-warnings / report', 'Report advisory warnings and verify collection completeness'],
]);
const WARNING_COLLECTIONS = new Map([
  ['Rust tests / rust tests (full suite)', 'tests-full-Linux-x86_64-unknown-linux-gnu'],
  ['Rust tests / rust platform (macOS)', 'tests-platform-macOS-aarch64-apple-darwin'],
  ['Rust tests / rust platform (Windows)', 'tests-platform-Windows-x86_64-pc-windows-msvc'],
  ['Rust tests / rust embeddings', 'tests-embeddings-macOS-aarch64-apple-darwin'],
]);
const baseName = name => [...TEST_STEPS.keys()].find(base => name === base || name.startsWith(base + ' ('));

export function parseTestWaiver(body = '') {
  const lines = body.split(/\r?\n/).filter(line => /^Release-Test-Waiver:/.test(line));
  if (!lines.length) return undefined;
  if (lines.length !== 1) throw new Error('Ambiguous release test waiver');
  return validateTestWaiver(JSON.parse(lines[0].slice('Release-Test-Waiver:'.length)));
}

export function validateTestWaiver(waiver) {
  if (!waiver || Object.keys(waiver).sort().join(',') !== 'jobs,run_id' ||
      !/^[1-9]\d*$/.test(waiver.run_id) || typeof waiver.run_id !== 'string' ||
      !Array.isArray(waiver.jobs) || !waiver.jobs.length || waiver.jobs.length > 100 ||
      new Set(waiver.jobs).size !== waiver.jobs.length ||
      waiver.jobs.some(name => typeof name !== 'string' || !baseName(name))) {
    throw new Error('Waiver must name only exact test jobs and one run ID; build/sign jobs cannot be waived');
  }
  return waiver;
}

export function waiverReleaseNotes(body, waiver) {
  if (!waiver) return body;
  validateTestWaiver(waiver);
  const clean = body.replace(/\n*<!-- release-test-waiver -->[\s\S]*?<!-- \/release-test-waiver -->/g, '').trimEnd();
  return clean + '\n\n<!-- release-test-waiver -->\n### Owner test waiver\n\n' +
    `rollacode authorized publication from qualification run ${waiver.run_id} with these defective test jobs waived:\n\n` +
    waiver.jobs.map(name => '- ' + name).join('\n') +
    '\n\nAll platform builds/signing and draft assets passed admission. Test fixes go to the next version.\n<!-- /release-test-waiver -->';
}

function onlyFailedSteps(job, names) {
  const failed = job.steps?.filter(step => step.conclusion === 'failure');
  if (job.status !== 'completed' || job.conclusion !== 'failure' || !failed?.length ||
      failed.some(step => !names.includes(step.name)) ||
      job.steps.some(step => step.status !== 'completed' || !['success', 'failure', 'skipped'].includes(step.conclusion))) {
    throw new Error(`Unrelated failure in ${job.name}`);
  }
}

export function verifyTestWaiver({ waiver, run, jobs, attempts, sha, repository, tag, warningLog = '', draft, assets, receipts,
  now = Date.now(), publicationRecovery = false, admission }) {
  validateTestWaiver(waiver);
  if (!isReleasePushRun(run, sha, repository) || String(run.id) !== waiver.run_id ||
      run.status !== 'completed' || run.conclusion !== 'failure' ||
      !Number.isFinite(Date.parse(run.updated_at)) || now - Date.parse(run.updated_at) > 3 * 86400_000 ||
      Date.parse(run.updated_at) > now) throw new Error('Foreign, expired or unfinished waived run');
  const effective = latestQualificationJobs(jobs, run, sha);
  const named = effective.filter(job => waiver.jobs.includes(job.name));
  if (named.length !== waiver.jobs.length) throw new Error('Missing named waived test job');
  for (const job of named) onlyFailedSteps(job, TEST_STEPS.get(baseName(job.name)));
  const rustFailed = named.some(job => job.name.startsWith('Rust tests / '));
  const derived = [];
  for (const job of effective) {
    if (waiver.jobs.includes(job.name) || job.conclusion === 'success') continue;
    if (job.name === 'qualification receipt' && job.status === 'completed' && job.conclusion === 'skipped') {
      derived.push(job); continue;
    }
    if (REPORT_STEPS.has(job.name) && (job.name === 'CI result' || rustFailed)) {
      onlyFailedSteps(job, [REPORT_STEPS.get(job.name)]);
      if (job.name === 'rust-warnings / report') {
        const incomplete = [...warningLog.matchAll(/(?:^|\n)(?:\S+ )?Rust warning evidence incomplete: ([^\r\n]+)/g)];
        const allowed = named.map(job => WARNING_COLLECTIONS.get(baseName(job.name)));
        if (/Rust warning evidence missing:/.test(warningLog) || incomplete.length !== 1 ||
            incomplete[0][1].split(', ').some(collection => !allowed.includes(collection))) {
          throw new Error('Warning report has unrelated or unverifiable incomplete evidence');
        }
      }
      derived.push(job); continue;
    }
    if (job.conclusion !== 'skipped' || job.status !== 'completed') throw new Error(`Unwaived qualification failure: ${job.name}`);
  }
  for (const prefix of [...QUALIFICATION_JOBS, 'rust-warnings / report',
    ...REQUIRED_JOBS.map(name => 'Release packaging / ' + name)]) {
    const matching = effective.filter(job => job.name === prefix || job.name.startsWith(prefix.endsWith(' / ') ? prefix : prefix + ' ('));
    if (!matching.length || matching.some(job => job.conclusion !== 'success' && !named.includes(job) && !derived.includes(job))) {
      throw new Error(`Missing green unwaived job: ${prefix}`);
    }
  }
  const green = effective.filter(job => job.conclusion === 'success');
  resolveQualificationOrigins(green, jobs, run, attempts, sha, repository);
  if (!/^v\d+\.\d+\.\d+$/.test(tag ?? '')) throw new Error('Missing package version for waived run');
  const recordedWaiver = { ...waiver, actor: 'rollacode', derived_jobs: derived.map(job => ({ name: job.name, conclusion: job.conclusion })) };
  const recovering = publicationRecovery && draft?.draft === false && draft.tag_name === tag &&
    admission?.sha === sha && admission.repository === repository && String(admission.run_id) === String(run.id) &&
    admission.run_attempt === run.run_attempt && JSON.stringify(admission.test_waiver) === JSON.stringify(recordedWaiver);
  if ((!draft?.draft && !recovering) || ![`qualification-${tag}-${run.id}`, tag].includes(draft.tag_name) ||
      draft.target_commitish !== sha || !draft.body?.includes(`<!-- qualification:${JSON.stringify({ source_sha: sha, run_id: String(run.id) })} -->`) ||
      !draft.body.includes('<!-- qualification-complete -->')) throw new Error('Missing complete own-run hidden draft');
  for (const platform of PLATFORMS) {
    const receipt = receipts.find(row => row.platform === platform);
    if (!receipt || receipts.filter(row => row.platform === platform).length !== 1 || receipt.schema !== 1 ||
        receipt.tag !== tag || receipt.source_sha !== sha || !receipt.files?.length) throw new Error('Missing qualified platform assets');
    verifyAssetDigests(receipt.files, assets, repository);
  }
  return { version: 2, sha, repository, workflow: run.path, run_id: run.id, run_attempt: run.run_attempt,
    test_waiver: recordedWaiver,
    verified_jobs: effective.map(job => ({ id: job.id, name: job.name, conclusion: job.conclusion, run_attempt: job.run_attempt })) };
}

export function* testWaiverEvidence(sha, waiver, repository, publicationRecovery = false) {
  validateTestWaiver(waiver);
  const root = `repos/${repository}`;
  const run = yield { route: `${root}/actions/runs/${waiver.run_id}` };
  if (!isReleasePushRun(run, sha, repository) || String(run.id) !== waiver.run_id) throw new Error('Foreign waived run');
  const manifest = yield { route: `${root}/contents/package.json?ref=${sha}` };
  if (manifest.encoding !== 'base64' || typeof manifest.content !== 'string') throw new Error('Missing package.json for waived run');
  const tag = releaseTag(JSON.parse(Buffer.from(manifest.content, 'base64').toString('utf8')));
  const jobs = [];
  for (let page = 1; ; page++) {
    if (page > 20) throw new Error('Waiver job inventory exceeds bound');
    const batch = yield { route: `${root}/actions/runs/${run.id}/jobs?filter=all&per_page=100&page=${page}` };
    jobs.push(...batch.jobs);
    if (jobs.length >= batch.total_count) break;
    if (!batch.jobs.length) throw new Error('Incomplete waiver job inventory');
  }
  const attempts = [];
  if (!Number.isSafeInteger(run.run_attempt) || run.run_attempt < 1 || run.run_attempt > 20) throw new Error('Invalid waived attempt');
  for (let number = 1; number <= run.run_attempt; number++) {
    attempts.push(yield { route: `${root}/actions/runs/${run.id}/attempts/${number}` });
  }
  let warningLog = '';
  const warningJob = latestQualificationJobs(jobs, run, sha).find(job => job.name === 'rust-warnings / report');
  if (warningJob?.conclusion === 'failure') warningLog = yield { route: `${root}/actions/jobs/${warningJob.id}/logs`, raw: true };
  let draft = yield { route: `${root}/releases/tags/qualification-${tag}-${run.id}`, optional: true };
  if (!draft) draft = yield { route: `${root}/releases/tags/${tag}` };
  const assets = [];
  for (let page = 1; ; page++) {
    if (page > 10) throw new Error('Draft asset inventory exceeds bound');
    const batch = yield { route: `${root}/releases/${draft.id}/assets?per_page=100&page=${page}` };
    assets.push(...batch);
    if (batch.length < 100) break;
  }
  const receipts = [];
  for (const platform of PLATFORMS) {
    const matches = assets.filter(asset => asset.name === `qualification-${platform}.json`);
    if (matches.length !== 1) throw new Error('Missing platform draft completion receipt');
    receipts.push(JSON.parse(yield { route: `${root}/releases/assets/${matches[0].id}`, raw: true }));
  }
  let admission;
  if (!draft.draft && publicationRecovery) {
    const matches = assets.filter(asset => asset.name === 'release-admission.json');
    if (matches.length !== 1) throw new Error('Missing published release admission for waiver recovery');
    admission = JSON.parse(yield { route: `${root}/releases/assets/${matches[0].id}`, raw: true });
  }
  return verifyTestWaiver({ waiver, run, jobs, attempts, sha, repository, tag, warningLog, draft, assets, receipts, publicationRecovery, admission });
}
