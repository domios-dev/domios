import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { stripVTControlCharacters } from 'node:util';
import { pathToFileURL } from 'node:url';
import { StringDecoder } from 'node:string_decoder';

export function failureDetector(onFailure, identity = {}, seen = new Set()) {
  let pending = '';
  const decoder = new StringDecoder('utf8');
  const line = (raw) => {
    const match = stripVTControlCharacters(raw).match(/^\s*(FAIL|TIMEOUT)\s+\[[^\]]*\]\s+(.+?)\s*$/);
    if (!match) return;
    const test = match[2];
    const key = JSON.stringify([identity.run, identity.attempt, identity.job, test]);
    if (seen.has(key)) return;
    seen.add(key);
    onFailure({ test, exit: match[1] === 'TIMEOUT' ? 124 : 1 });
  };
  return {
    write(chunk) {
      pending += typeof chunk === 'string' ? chunk : decoder.write(chunk);
      const lines = pending.split(/\r?\n|\r(?!$)/);
      pending = lines.pop();
      for (const value of lines) line(value);
      if (pending.length > 65536) pending = pending.slice(-65536);
    },
    end() { line(pending + decoder.end()); pending = ''; },
  };
}

export function failureEvent(env, detail = {}) {
  return {
    repo: env.GITHUB_REPOSITORY,
    run: env.GITHUB_RUN_ID,
    attempt: env.GITHUB_RUN_ATTEMPT,
    sha: env.CI_SOURCE_SHA || env.GITHUB_SHA,
    job: env.RUNNER_NAME ? `${env.GITHUB_JOB}/${env.RUNNER_NAME}` : env.GITHUB_JOB,
    step: env.CI_FAILURE_STEP || (detail.test ? 'Rust test execution' : 'job'),
    test: detail.test ?? null,
    exit: detail.exit ?? (env.CI_FAILURE_EXIT ? Number(env.CI_FAILURE_EXIT) : null),
    log_url: `${env.GITHUB_SERVER_URL || 'https://github.com'}/${env.GITHUB_REPOSITORY}/actions/runs/${env.GITHUB_RUN_ID}/attempts/${env.GITHUB_RUN_ATTEMPT}`,
    ...detail,
  };
}

export function failureAnnotation(event) {
  const message = `${event.job}: ${event.test || event.step} failed; cancelling run ${event.run} attempt ${event.attempt}`;
  return `::error title=CI test failure::${message.replaceAll('%', '%25').replaceAll('\r', '%0D').replaceAll('\n', '%0A')}\n`;
}

function cancelRun(env) {
  return new Promise((resolve, reject) => {
    const child = spawn('gh', ['run', 'cancel', env.GITHUB_RUN_ID, '--repo', env.GITHUB_REPOSITORY],
      { env, stdio: ['ignore', 'ignore', 'ignore'] });
    const deadline = setTimeout(() => { child.kill(); reject(new Error('cancel deadline exceeded')); }, 5000);
    child.on('error', reject);
    child.on('close', (code) => {
      clearTimeout(deadline);
      if (code === 0) resolve(); else reject(new Error(`gh run cancel exited ${code}`));
    });
  });
}

export async function firstFailure(detail = {}, {
  env = process.env, fetchEvent = fetch, cancel = cancelRun,
  claim = (key) => {
    const directory = path.join(env.RUNNER_TEMP, `domios-ci-failure-${key}`);
    try { fs.mkdirSync(directory); return true; } catch (error) {
      if (error.code === 'EEXIST') return false;
      throw error;
    }
  },
  warn = message => process.stderr.write(`::warning::${message}\n`),
  announce = event => process.stderr.write(failureAnnotation(event)),
} = {}) {
  if (env.CI_CANCEL_RUN !== 'true' || !env.GITHUB_RUN_ID || !env.GITHUB_REPOSITORY) return;
  const event = failureEvent(env, detail);
  try {
    const key = [event.repo, event.run, event.attempt, event.job].join('-').replace(/[^\w.-]/g, '_');
    if (!claim(key)) return;
  } catch { warn('CI failure deduplication unavailable; continuing cancellation'); }
  announce(event);
  try {
    if (!env.DOMIOS_CI_WEBHOOK_URL || !env.DOMIOS_CI_WEBHOOK_TOKEN) throw new Error('not configured');
    const response = await fetchEvent(env.DOMIOS_CI_WEBHOOK_URL, {
      method: 'POST', redirect: 'error',
      headers: { Authorization: `Bearer ${env.DOMIOS_CI_WEBHOOK_TOKEN}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(event), signal: AbortSignal.timeout(3000),
    });
    if (!response.ok) throw new Error('receiver refused event');
  } catch { warn('CI failure notification unavailable; original failure retained'); }
  try { await cancel(env); } catch { warn('CI run cancellation unavailable; original failure retained'); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  let steps = {};
  try { steps = JSON.parse(process.env.CI_FAILURE_STEPS || '{}'); } catch { /* Preserve the failing job. */ }
  const step = Object.entries(steps).find(([, value]) => value.outcome === 'failure')?.[0];
  await firstFailure(step ? { step } : {});
}
