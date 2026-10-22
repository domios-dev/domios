#!/usr/bin/env node
// Caller: CI release.yml (tests-tier, builds-tier)
// Opens the next release tier once every job of the previous tier is claimed by a runner or finished.
import { execFileSync } from 'node:child_process';
import { appendFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const UNCLAIMED = new Set(['queued', 'waiting', 'pending', 'requested']);

// The anchor is the tier's first job: its dependents are created only when it finishes.
export function tierState(jobs, { prefix, anchor }) {
  const tier = jobs.filter(job => job.name.startsWith(prefix));
  const first = tier.find(job => job.name === anchor);
  if (!first) return { open: false, reason: `${anchor} has not started` };
  if (first.status !== 'completed') return { open: false, reason: `${anchor} is ${first.status}` };
  const unclaimed = tier.filter(job => UNCLAIMED.has(job.status));
  if (unclaimed.length) return { open: false, reason: `unclaimed: ${unclaimed.map(job => job.name).join(', ')}` };
  return { open: true, reason: `${tier.length} job(s) of "${prefix.trim()}" claimed or finished` };
}

// GitHub has no event for "a runner claimed a job", so the gate reads the run's jobs on a bounded cadence.
// Two consecutive open readings absorb the moment between an anchor finishing and its dependents appearing.
export async function waitForTier({ fetchJobs, sleep, now, prefix, anchor, intervalMs, deadlineMs, log = console.log }) {
  const start = now();
  let openReadings = 0;
  for (;;) {
    const state = tierState(await fetchJobs(), { prefix, anchor });
    openReadings = state.open ? openReadings + 1 : 0;
    if (openReadings >= 2) return { opened: true, reason: state.reason };
    if (now() - start >= deadlineMs) return { opened: false, reason: state.reason };
    log(`waiting: ${state.reason}`);
    await sleep(intervalMs);
  }
}

function fetchRunJobs(repository, runId) {
  // Runner gh builds predate --slurp; --jq emits one object per line across pages.
  return execFileSync('gh', ['api', '--paginate', '--jq', '.jobs[] | {name, status} | tojson',
    `repos/${repository}/actions/runs/${runId}/jobs?per_page=100`], { encoding: 'utf8' })
    .split('\n').filter(Boolean).map(line => JSON.parse(line));
}

async function main([prefix, anchor, deadlineMinutes = '45']) {
  if (!prefix || !anchor) throw new Error('usage: release-tier-gate.mjs <previous-tier job prefix> <anchor job name> [deadline minutes]');
  const repository = process.env.GITHUB_REPOSITORY;
  const runId = process.env.GITHUB_RUN_ID;
  const started = Date.now();
  const result = await waitForTier({
    fetchJobs: () => fetchRunJobs(repository, runId),
    sleep: ms => new Promise(resolve => setTimeout(resolve, ms)),
    now: () => Date.now(),
    prefix, anchor, intervalMs: 15_000, deadlineMs: Number(deadlineMinutes) * 60_000,
  });
  // Priority is a scheduling preference: a tier that never gets claimed must not deadlock the lower tiers,
  // but the fallback is always visible in the job log, the run summary and the stored tier receipt.
  const receipt = { tier_above: prefix.trim(), anchor, opened_on_schedule: result.opened, reason: result.reason, waited_seconds: Math.round((Date.now() - started) / 1000) };
  if (result.opened) console.log(`tier open: ${result.reason}`);
  else console.log(`::warning::tier order fallback: opening the next tier after ${deadlineMinutes} min while "${prefix.trim()}" still has ${result.reason}`);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `Tier gate after "${prefix.trim()}": ${result.opened ? 'opened in order' : '**FALLBACK** ' + result.reason} (${receipt.waited_seconds} s)\n`);
  const file = path.join(process.env.RUNNER_TEMP || '.', `tier-${receiptName(prefix)}.json`);
  writeFileSync(file, JSON.stringify(receipt, null, 2));
  execFileSync('bash', ['scripts/ci-artifact.sh', 'put', `${runId}/release-tiers/${receiptName(prefix)}.json`, file], { stdio: 'inherit' });
}

export const receiptName = prefix => prefix.trim().replace(/[^a-z0-9]+/gi, '-').replace(/^-|-$/g, '').toLowerCase();

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch(error => { console.error(`::error::${error.message}`); process.exitCode = 1; });
}
