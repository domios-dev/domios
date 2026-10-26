import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function ciFailures(event, needs, scope = '', { releaseHead = false, skipHeavy = false } = {}) {
  const required = ['policy', 'fast'];
  if (skipHeavy) required.splice(required.indexOf('fast'), 1);
  if (releaseHead) required.push('rust', 'rust-warnings', 'qualification', 'release-dry-run');
  const failures = new Set(required.filter(id => needs[id]?.result !== 'success'));
  for (const [id, job] of Object.entries(needs)) {
    if (job.result === 'failure' || job.result === 'cancelled') failures.add(id);
  }
  if (event === 'workflow_dispatch' && scope === 'fast') {
    for (const id of ['rust', 'rust-warnings', 'qualification']) {
      if (needs[id]?.result !== 'skipped') failures.add(id);
    }
  }
  return [...failures];
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const needs = JSON.parse(process.env.CI_NEEDS);
  const failures = ciFailures(process.env.CI_EVENT, needs, process.env.CI_SCOPE, {
    releaseHead: process.env.CI_RELEASE_HEAD === 'true', skipHeavy: process.env.CI_SKIP_HEAVY === 'true',
  });
  const label = process.env.CI_EVENT === 'workflow_dispatch' ? 'CI diagnostic scope: ' + (process.env.CI_SCOPE || 'full') + '; this run does not qualify the SHA.\n' : '';
  const summary = label + Object.entries(needs).map(([id, job]) => '- ' + id + ': ' + job.result).join('\n');
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary + '\n');
  if (failures.length) throw new Error('CI did not pass: ' + failures.join(', '));
}
