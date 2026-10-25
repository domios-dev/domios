import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { condition } from './workflow-condition.mjs';

const root = new URL('../../', import.meta.url);
const read = file => readFileSync(new URL(file, root), 'utf8');
const workflows = Object.fromEntries(readdirSync(new URL('.github/workflows/', root))
  .filter(file => file.endsWith('.yml')).map(file => [file, parse(read('.github/workflows/' + file))]));
const ci = workflows['tests.yml'];

test('one CI event entrypoint owns every guard and its aggregate is the only required check', () => {
  const pr = Object.entries(workflows).filter(([, w]) => Object.hasOwn(w.on, 'pull_request')).map(([name]) => name);
  assert.deepEqual(pr, ['tests.yml']);
  for (const [name, w] of Object.entries(workflows)) {
    if (w.on.push?.branches?.some(branch => ['develop', 'rename/domios', 'master', 'release/**'].includes(branch))) {
      assert.equal(name, 'tests.yml');
    }
  }
  const dependencies = new Set(ci.jobs.result.needs);
  assert.deepEqual(dependencies, new Set(Object.keys(ci.jobs).filter(id => id !== 'result')));
  assert.equal(condition(ci.jobs.result.if, { failed: true }), true);
  const admin = read('.github/admin/public-repo.sh');
  const checks = [...admin.matchAll(/required_status_checks:\[(.*?)\]/g)];
  assert.ok(checks.length);
  for (const [, contexts] of checks) {
    const required = [...contexts.matchAll(/context:"([^"]+)"/g)].map(match => match[1]);
    assert.deepEqual(required, [ci.jobs.result.name]);
  }
  const commands = ci.jobs.policy.steps.map(step => step.run || '').join('\n');
  for (const guard of ['check-file-sizes.sh', 'check-provider-boundary.mjs', 'check-tauri-reach.sh',
    'check-crate-layering.sh', 'check-comment-length.sh', 'check-api-docs.sh',
    'check-prompts-doc.sh', 'check-evals-list.sh', 'secret-scan.sh', 'release_issue_contract.py']) {
    assert.ok(commands.includes(guard), guard);
  }
  const master = ci.jobs.policy.steps.find(step => step.name === 'Require a release branch');
  for (const event_name of ['push', 'pull_request']) for (const base_ref of ['master', 'develop']) {
    assert.equal(condition(master.if, { github: { event_name, base_ref } }),
      event_name === 'pull_request' && base_ref === 'master');
  }
});

test('runner preflight precedes compile and dependency work in every CI role', () => {
  for (const name of ['tests-fast.yml', 'tests-rust.yml', 'tests-pr.yml']) {
    for (const [id, job] of Object.entries(workflows[name].jobs)) {
      const preflight = job.steps.findIndex(step => step.run?.includes('ci-runner-paths.mjs'));
      assert.ok(preflight >= 0, name + '/' + id);
      const work = job.steps.findIndex(step => /pnpm install|cargo test|ci-tests|ci-cli-tests|ci-clippy|types:check/.test(step.run || ''));
      assert.ok(work < 0 || preflight < work, name + '/' + id);
    }
  }
});

test('hosted TS jobs never inherit credentials or private artifact storage', () => {
  for (const name of ['tests-fast.yml', 'tests-pr.yml']) {
    const workflow = workflows[name];
    for (const job of Object.values(workflow.jobs)) {
      if (typeof job['runs-on'] !== 'string' || job.env?.CI_HOSTED) continue;
      assert.doesNotMatch(JSON.stringify({ env: workflow.env, job }), /secrets\.|ci-artifact\.sh|ci-artifact-spool/);
      assert.equal(job.environment, undefined);
      assert.notEqual(job.secrets, 'inherit');
    }
  }
});
