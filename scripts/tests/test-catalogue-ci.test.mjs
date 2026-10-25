import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { scopeForRun } from '../ci-scope.mjs';
import { selectRunners } from '../test-catalogue.mjs';
import { condition } from './workflow-condition.mjs';

const read = file => readFileSync(new URL('../../' + file, import.meta.url), 'utf8');
const workflows = Object.fromEntries(readdirSync(new URL('../../.github/workflows/', import.meta.url))
  .filter(file => file.endsWith('.yml')).map(file => [file, parse(read('.github/workflows/' + file))]));
const catalogue = JSON.parse(read('tests/catalogue.json'));
const overlay = JSON.parse(read('tests/catalogue.overlay.json'));
const plugins = JSON.parse(read('plugins/channel.json')).plugins;
const pr = workflows['tests-pr.yml'];
const github = { event_name: 'pull_request', head_ref: 'lane/example', ref: 'refs/pull/1/merge' };

function reached(files, outcomes = {}) {
  const scope = scopeForRun(files, plugins, false, true, false);
  const outputs = Object.fromEntries(Object.entries(scope).map(([key, value]) =>
    [key, key === 'plugins' ? JSON.stringify(value) : String(value)]));
  const needs = {};
  for (const [id, job] of Object.entries(pr.jobs)) {
    const dependencies = [job.needs ?? []].flat();
    const allowed = dependencies.every(id => needs[id]?.result === 'success') &&
      (!job.if || condition(job.if, { github, needs }));
    needs[id] = { result: allowed ? outcomes[id] || 'success' : 'skipped', outputs: id === 'changed' ? outputs : {} };
  }
  return needs;
}

function ownerErrors(cat, graph = workflows) {
  const selected = selectRunners(cat, overlay, { tier: 'pr' }).runners;
  const errors = [];
  for (const entry of selected) {
    if (entry.needs_live_agent || entry.needs_instance || entry.kind === 'e2e-plain' || /playwright/i.test(entry.runner)) {
      errors.push(entry.id + ': forbidden PR entry');
      continue;
    }
    const owned = (entry.ci || []).some(owner => {
      const [file, id] = owner.split(':');
      if (!['tests.yml', 'tests-pr.yml'].includes(file)) return false;
      const job = graph[file]?.jobs[id];
      if (!job) return false;
      const commands = JSON.stringify(job.steps).replaceAll('\\n', ' ').replaceAll(' --locked', '');
      if (commands.includes("ci-catalogue-command.mjs " + entry.id) || commands.includes("[\'" + entry.id + "\']")) return true;
      return entry.runner.split(' && ').every(command => commands.includes(command.replace(' --locked', '')));
    });
    if (!owned) errors.push(entry.id + ': no executing PR owner');
  }
  return errors;
}

test('every PR catalogue runner has an executing owner under the single CI entrypoint', () => {
  assert.equal(workflows['tests.yml'].jobs.pr.uses, './.github/workflows/tests-pr.yml');
  assert.deepEqual(ownerErrors(catalogue), []);
  const mutant = structuredClone(catalogue);
  mutant.runners.push({ id: 'orphan', tier: 'pr', runner: 'echo orphan' });
  assert.ok(ownerErrors(mutant).some(error => error.startsWith('orphan:')));
});

test('docs, frontend, Rust and plugin changes select their behavior checks without heavy PR suites', () => {
  for (const files of [['essences/vision.md'], ['src/app.ts'], ['cli/src/main.rs'],
    ['scripts/new-check.mjs'], ...plugins.map(plugin => ['plugins/' + plugin.path + '/plugin.js'])]) {
    const needs = reached(files);
    assert.equal(needs.guards.result, 'success');
    assert.equal(needs.changed.outputs.heavy, 'false');
    if (needs.changed.outputs.frontend === 'true') {
      for (const id of ['ts-unit', 'ts-vitest', 'ts-static', 'frontend-dist']) assert.equal(needs[id].result, 'success', id);
    }
    if (needs.changed.outputs.rust === 'true') {
      for (const id of ['cli-tests', 'ts-types', 'ts-plugins']) assert.equal(needs[id].result, 'success', id);
    }
  }
});

test('one guard failure preserves independent test results and every PR job has a bounded deadline', () => {
  const needs = reached(['src/app.ts', 'cli/src/main.rs'], { guards: 'failure' });
  for (const id of ['cli-tests', 'ts-unit', 'ts-vitest', 'frontend-dist']) assert.equal(needs[id].result, 'success', id);
  for (const job of Object.values(pr.jobs)) {
    assert.ok(job['timeout-minutes'] > 0 && job['timeout-minutes'] <= 30);
    assert.notEqual(job['continue-on-error'], true);
    assert.ok(!job.strategy || job.strategy['fail-fast'] !== true);
  }
});

test('PR catalogue refuses live agents, daemon fixtures and suspended Playwright', () => {
  for (const change of [{ needs_live_agent: true }, { needs_instance: true }, { runner: 'pnpm exec playwright test' }]) {
    const mutant = structuredClone(catalogue);
    Object.assign(mutant.runners.find(entry => entry.id === 'ts-unit'), change);
    assert.ok(ownerErrors(mutant).some(error => error.includes('forbidden PR entry')));
  }
});

test('removing an owned test command is detected', () => {
  const graph = structuredClone(workflows);
  graph['tests-pr.yml'].jobs['ts-unit'].steps = [];
  const mutant = structuredClone(catalogue);
  mutant.runners.find(entry => entry.id === 'ts-unit').ci = ['tests-pr.yml:ts-unit'];
  assert.ok(ownerErrors(mutant, graph).some(error => error.startsWith('ts-unit:')));
});
