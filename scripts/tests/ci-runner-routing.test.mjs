import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
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

// Owner decision: hosted compile jobs may use the private cache store; no other private value reaches them.
const cacheStore = ['AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'CI_STORE_TAILNET_ENDPOINT', 'CI_STORE_PUBLIC_ENDPOINT'];


test('shared workflow evaluator always supplies a complete GitHub context without authorizing missing identity', () => {
  assert.equal(githubContext({ repository_owner: 'fixture' }).event.workflow_run.head_repository.full_name, '');
  assert.equal(condition("github.repository_owner == 'rollacode'"), false);
  assert.equal(condition("github.event.workflow_run.actor.login == 'rollacode'"), false);
  assert.equal(condition("github.repository_owner == 'rollacode'", { github: { repository_owner: 'rollacode' } }), true);
});


