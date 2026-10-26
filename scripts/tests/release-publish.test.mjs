import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { condition } from './workflow-condition.mjs';

const read = relative => readFileSync(new URL(`../../${relative}`, import.meta.url), 'utf8');
const workflow = parse(read('.github/workflows/release-publish.yml'));
const job = workflow.jobs.publish;



test('publication runs one script with release tokens and changes nothing in a rehearsal', () => {
  assert.equal(job.env.GH_TOKEN, '${{ secrets.GH_RELEASE_TOKEN }}');
  assert.equal(job.env.MIRROR_TOKEN, '${{ secrets.PACKAGE_REPOS_TOKEN }}');
  assert.match(job.steps.at(-1).run, /^bash scripts\/release-publish\.sh /);
  const script = read('scripts/release-publish.sh');
  const rehearsalEnd = script.indexOf('exit 0', script.indexOf('if [ "$MODE" = rehearsal ]'));
  for (const mutation of ['gh release create', 'gh release upload', 'gh release edit', 'git push', 'release-promote.mjs publish']) {
    const at = script.indexOf(mutation);
    assert.ok(at > rehearsalEnd, `${mutation} runs only after the rehearsal returned`);
  }
  assert.doesNotMatch(script, /--slurp/);
});
