import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function ciFailures(event, needs) {
  const required = ['policy', event === 'pull_request' ? 'pr' : 'fast'];
  if (event === 'push' || event === 'schedule') {
    if (needs.fast?.outputs?.heavy === 'true') required.push('rust', 'rust-warnings', 'qualification');
  }
  const failures = new Set(required.filter(id => needs[id]?.result !== 'success'));
  for (const [id, job] of Object.entries(needs)) {
    if (job.result === 'failure' || job.result === 'cancelled') failures.add(id);
  }
  return [...failures];
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const needs = JSON.parse(process.env.CI_NEEDS);
  const failures = ciFailures(process.env.CI_EVENT, needs);
  const details = fs.existsSync('ci-job-failures.txt') ? fs.readFileSync('ci-job-failures.txt', 'utf8').trim() : '';
  const summary = Object.entries(needs).map(([id, job]) => '- ' + id + ': ' + job.result).join('\n') + (details ? '\n' + details : '');
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary + '\n');
  if (failures.length) throw new Error('CI did not pass: ' + failures.join(', ') + (details ? '\n' + details : ''));
}
