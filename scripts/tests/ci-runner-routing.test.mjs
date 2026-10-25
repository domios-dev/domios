import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { condition, evaluateCondition, githubContext } from './workflow-condition.mjs';

const workflow = name => parse(readFileSync(new URL('../../.github/workflows/' + name + '.yml', import.meta.url), 'utf8'));
const switches = { CI_LINUX_LIGHT_RUNNER: 'ubuntu-24.04', CI_LINUX_RUST_COMPILE_RUNNER: 'ubuntu-24.04',
  CI_LINUX_RUST_FULL_RUNNER: 'ubuntu-24.04', CI_MACOS_RUST_RUNNER: 'macos-15', CI_MACOS_EMBEDDINGS_RUNNER: 'macos-15' };
const secretValues = new Proxy({}, { get: (_, key) => 'PRIVATE:' + String(key) });
const value = (expression, context) => {
  if (typeof expression !== 'string') return expression;
  if (expression.startsWith('$' + '{{') && expression.endsWith('}}')) return evaluateCondition(expression, context);
  return expression.replace(/\$\{\{([\s\S]*?)\}\}/g, (_, source) => String(evaluateCondition(source, context)));
};

test('runner switches preserve private defaults and hosted variants expose no private credential or writable token', () => {
  for (const name of ['tests-fast', 'tests-rust', 'tests', 'rust-warnings-summary']) {
    const graph = workflow(name);
    for (const [id, job] of Object.entries(graph.jobs)) {
      if (!job.env?.CI_HOSTED) continue;
      const rows = job.strategy?.matrix?.include || [{}];
      for (const matrix of rows) {
        const needs = new Proxy({}, { get: () => ({ result: 'success', outputs: new Proxy({}, { get: () => '' }) }) });
        const steps = new Proxy({}, { get: () => ({ outputs: new Proxy({}, { get: () => '' }) }) });
        const base = { matrix, needs, steps, github: { workflow: 'CI', token: 'read-only-runtime-token' },
          secrets: secretValues, runner: { os: matrix.os || 'Linux', arch: 'X64' } };
        const defaults = value(job['runs-on'], { ...base, vars: {} });
        assert.ok(defaults.includes('self-hosted'), name + '/' + id);
        const context = { ...base, vars: switches };
        if (!condition(job.env.CI_HOSTED, context)) continue;
        const runner = value(job['runs-on'], context);
        assert.ok(['ubuntu-24.04', 'macos-15'].includes(runner));
        assert.equal(value(job.environment, context), '');
        assert.ok(Object.values(job.permissions || {}).every(permission => permission !== 'write'));
        for (const step of [{ env: {} }, ...job.steps]) {
          const effective = { ...graph.env, ...job.env, ...step.env };
          for (const [key, expression] of Object.entries(effective)) {
            assert.ok(!String(value(expression, context)).includes('PRIVATE:'), name + '/' + id + '/' + key);
          }
        }
        if (job.env.SCCACHE_GHA_ENABLED) {
          assert.equal(value(job.env.SCCACHE_GHA_ENABLED, context), 'true');
          assert.ok(job.steps.some(step => step.uses?.startsWith('mozilla-actions/sccache-action@')));
          const cache = job.steps.find(step => step.uses?.startsWith('actions/cache@'));
          assert.ok(cache && !cache.with.path.includes('target'));
        }
      }
    }
  }
});

test('shared workflow evaluator always supplies a complete GitHub context without authorizing missing identity', () => {
  assert.equal(githubContext({ repository_owner: 'fixture' }).event.workflow_run.head_repository.full_name, '');
  assert.equal(condition("github.repository_owner == 'rollacode'"), false);
  assert.equal(condition("github.event.workflow_run.actor.login == 'rollacode'"), false);
  assert.equal(condition("github.repository_owner == 'rollacode'", { github: { repository_owner: 'rollacode' } }), true);
});

test('macOS signing and notarization cannot use the non-signing hosted runner switches', () => {
  const signing = workflow('release-macos-build').jobs.build;
  assert.match(JSON.stringify(signing['runs-on']), /self-hosted/);
  assert.doesNotMatch(JSON.stringify(signing['runs-on']), /CI_MACOS_|macos-15/);
  assert.equal(signing.environment, 'release');
});
