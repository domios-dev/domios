import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { RUNNER_ROLES } from '../ci-runner-paths.mjs';

const providers = {
  node: 'actions/setup-node@',
  python: 'actions/setup-python@',
  pnpm: 'pnpm/action-setup@',
};
const pinned = (step, tool) => step.uses?.startsWith(providers[tool]) &&
  /@[a-f0-9]{40}$/.test(step.uses) && !step.if && !step['continue-on-error'];

function orderingErrors(steps) {
  const ready = new Set();
  const errors = [];
  for (const step of steps) {
    const command = step.run || '';
    const required = new Set([...command.matchAll(/\b(node|pnpm|python3?)\b/g)]
      .map(match => match[1].startsWith('python') ? 'python' : match[1]));
    for (const [, role = 'rust'] of command.matchAll(/node\s+scripts\/ci-runner-paths\.mjs(?:\s+([\w-]+))?/g)) {
      for (const tool of RUNNER_ROLES[role] || []) if (tool in providers) required.add(tool);
    }
    if (step.uses?.startsWith(providers.pnpm)) required.add('node');
    for (const tool of required) if (!ready.has(tool)) errors.push((step.name || step.id || command) + ': ' + tool + ' used before unconditional pinned setup');
    for (const tool of Object.keys(providers)) if (pinned(step, tool)) ready.add(tool);
  }
  return errors;
}

test('every executable workflow job provisions pinned Node before any tool-dependent step', () => {
  const directory = new URL('../../.github/workflows/', import.meta.url);
  let jobs = 0;
  for (const file of readdirSync(directory).filter(file => file.endsWith('.yml'))) {
    const workflow = parse(readFileSync(new URL(file, directory), 'utf8'));
    for (const [id, job] of Object.entries(workflow.jobs)) {
      if (!job.steps) continue;
      jobs++;
      assert.ok(pinned(job.steps[0], 'node'), file + '/' + id + ': Node bootstrap is first');
      assert.match(job.steps[0].with['node-version'], /^\d+\.\d+\.\d+$/);
      assert.equal(job.steps[0].with['package-manager-cache'], false);
      assert.deepEqual(orderingErrors(job.steps), [], file + '/' + id);
    }
  }
  assert.ok(jobs > 0);
});

test('regression: preflight on a clean service PATH cannot precede its Node/Python/pnpm providers', () => {
  const setup = tool => ({ uses: providers[tool] + 'a'.repeat(40) });
  const preflight = { run: 'node scripts/ci-runner-paths.mjs pr-guards' };
  const good = [setup('node'), setup('python'), setup('pnpm'), preflight];
  assert.deepEqual(orderingErrors(good), []);
  for (const tool of Object.keys(providers)) {
    const mutant = good.filter(step => step !== good.find(step => pinned(step, tool)));
    assert.ok(orderingErrors(mutant).some(error => error.includes(tool + ' used before')));
  }
  assert.ok(orderingErrors([preflight, ...good]).some(error => error.includes('node used before')));
  assert.ok(orderingErrors([{ ...setup('node'), if: 'steps.tree.outputs.run' }, { run: 'node fixture.mjs' }]).length);
});

test('regression: direct Python/pnpm commands and the pnpm setup action require their providers', () => {
  assert.ok(orderingErrors([{ run: 'python3 fixture.py' }]).length);
  assert.ok(orderingErrors([{ run: 'pnpm install' }]).length);
  assert.ok(orderingErrors([{ uses: providers.pnpm + 'a'.repeat(40) }]).some(error => error.includes('node used before')));
});
