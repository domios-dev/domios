import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { condition } from './workflow-condition.mjs';

const read = relative => readFileSync(new URL(`../../${relative}`, import.meta.url), 'utf8');
const workflow = parse(read('.github/workflows/release-publish.yml'));
const job = workflow.jobs.publish;

test('a dispatch publishes or rehearses one named dry run; rehearsal is the default', () => {
  const inputs = workflow.on.workflow_dispatch.inputs;
  assert.equal(inputs.run_id.required, true);
  assert.equal(inputs.tag.required, true);
  assert.equal(inputs.rehearsal.default, true);
  assert.equal(condition(job.if, { github: { event_name: 'workflow_dispatch' }, vars: {} }), true);
  assert.deepEqual(workflow.concurrency, { group: 'release-publication', 'cancel-in-progress': false });
});

test('a green release-branch dry run publishes itself only while RELEASE_AUTO_PROMOTE is on', () => {
  assert.deepEqual(workflow.on.workflow_run, { workflows: ['Release'], types: ['completed'] });
  const event = (conclusion, head_branch) => ({ event_name: 'workflow_run', event: { workflow_run: { conclusion, head_branch } } });
  const on = { RELEASE_AUTO_PROMOTE: 'true' };
  assert.equal(condition(job.if, { github: event('success', 'release/9.9.9'), vars: on }), true);
  assert.equal(condition(job.if, { github: event('success', 'release/9.9.9'), vars: {} }), false);
  assert.equal(condition(job.if, { github: event('failure', 'release/9.9.9'), vars: on }), false);
  assert.equal(condition(job.if, { github: event('success', 'develop'), vars: on }), false);
  const plan = job.steps.find(step => step.id === 'plan').run;
  assert.match(plan, /release-promote\.mjs check/);
  assert.match(plan, /mode=skip/);
});

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
