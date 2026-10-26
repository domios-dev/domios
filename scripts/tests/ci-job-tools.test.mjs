import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { parse } from 'yaml';
import { jobTools, commandTools } from '../ci-job-tools.mjs';
import { condition } from './workflow-condition.mjs';
import { runnerKinds } from './ci-runner-scenarios.mjs';
import { runnerRequirements } from '../ci-runner-paths.mjs';

const read = name => parse(fs.readFileSync(new URL('../../.github/workflows/' + name + '.yml', import.meta.url), 'utf8'));
const files = ['tests', 'tests-fast', 'tests-rust', 'tests-pr'];
function provisionErrors(job, kind='github-hosted') {
  const errors = [];
  const steps = job.steps;
  const preflight = steps.findIndex(step => step.name === 'Runner preflight');
  if (preflight < 0) return JSON.stringify(jobTools(job)) === '["node"]' ? [] : ['missing preflight'];
  const required = jobTools(job);
  const declared = runnerRequirements(steps[preflight].run.split(/\s+/).at(-1));
  if (JSON.stringify(required) !== JSON.stringify(declared)) errors.push('declared tools differ from command consumers');
  const before = steps.slice(0, preflight);
  const enabled = step => !step.if || (step.if.includes('runner.environment') && condition(step.if,{runner:{environment:kind}}));
  const discovered = kind==='self-hosted' && before.some(step=>step.run==='node scripts/ci-runner-paths.mjs --paths-only' && enabled(step));
  const pinned = prefix => before.some(step => step.uses?.startsWith(prefix) && /@[a-f0-9]{40}$/.test(step.uses) && enabled(step));
  for (const tool of ['node', 'python', 'pnpm']) {
    const prefix = tool === 'pnpm' ? 'pnpm/action-setup@' : 'actions/setup-' + tool + '@';
    if (required.includes(tool) && !pinned(prefix) && !(kind==='self-hosted' && (tool==='node' ? pinned('actions/github-script@') : discovered))) errors.push(tool + ' not provisioned before preflight');
  }
  if(required.includes('cargo') && kind==='github-hosted' && !pinned('dtolnay/rust-toolchain@'))errors.push('pinned Rust toolchain not provisioned before preflight');
  const nextest = before.find(step => step.uses?.startsWith('taiki-e/install-action@') && step.with?.tool === 'cargo-nextest@0.9.148' && enabled(step));
  if (required.includes('nextest') && !nextest && kind==='github-hosted') errors.push('nextest not installed before preflight');
  if (!required.includes('nextest') && steps.some(step => step.with?.tool?.startsWith('cargo-nextest@'))) errors.push('unused nextest installation');
  if (required.includes('nextest') && kind==='github-hosted') {
    const installedAt = steps.indexOf(nextest);
    if (steps.some((step,index) => commandTools(step.run || '').includes('nextest') && index < installedAt)) errors.push('nextest invoked before installation');
  }
  if (required.includes('python:yaml') && !before.some(step =>
    step.run === 'node scripts/ci-python-env.mjs' && !step.if)) errors.push('Python modules not installed before preflight');
  return errors;
}



test('entrypoint semantics distinguish compilation, Cargo tests, nextest execution and receipt verification', () => {
  assert.ok(!commandTools('bash build.sh ci-clippy').includes('nextest'));
  assert.ok(!commandTools('bash build.sh ci-cli-tests').includes('nextest'));
  assert.ok(!commandTools('pnpm types:check').includes('nextest'));
  assert.ok(commandTools('pnpm types:check').includes('cargo'));
  assert.ok(commandTools('bash build.sh ci-tests platform').includes('nextest'));
  assert.ok(commandTools('cargo nextest run --workspace').includes('nextest'));
  assert.ok(!commandTools('node scripts/ci-rust-tests.mjs verify ci-rust-receipts').includes('nextest'));
});

