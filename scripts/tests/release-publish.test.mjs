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
  assert.equal(condition(job.if, { github: { event_name: 'workflow_dispatch', actor: 'rollacode', triggering_actor: 'rollacode', repository: 'rollacode/codeterm', repository_owner: 'rollacode', ref: 'refs/heads/release/9.9.9' }, vars: {} }), true);
  for (const change of [{ repository_owner: 'fork' }, { actor: 'contributor' },
    { triggering_actor: 'contributor' }, { ref: 'refs/heads/master' }]) {
    assert.equal(condition(job.if, { github: { event_name: 'workflow_dispatch', actor: 'rollacode',
      triggering_actor: 'rollacode', repository_owner: 'rollacode', ref: 'refs/heads/release/9.9.9', ...change } }), false);
  }
  assert.deepEqual(workflow.concurrency, { group: 'release-publication', 'cancel-in-progress': false });
});

test('workflow completion cannot auto-publish retained bytes', () => {
  assert.equal(workflow.on.workflow_run, undefined);
  assert.equal(workflow.on.push, undefined);
  assert.equal(condition(job.if, { github: { event_name: 'workflow_run', actor: 'rollacode', triggering_actor: 'rollacode', repository: 'rollacode/codeterm', repository_owner: 'rollacode', ref: 'refs/heads/release/9.9.9' } }), false);
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
