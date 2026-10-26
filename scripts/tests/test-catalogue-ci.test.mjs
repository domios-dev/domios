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
      if (commands.includes("ci-catalogue-command.mjs " + entry.id) || new RegExp("ci-step\\.mjs " + entry.id + "(\\s|$)").test(commands) || commands.includes("[\'" + entry.id + "\']")) return true;
      return entry.runner.split(' && ').every(command => commands.includes(command.replace(' --locked', '')));
    });
    if (!owned) errors.push(entry.id + ': no executing PR owner');
  }
  return errors;
}


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


test('PR catalogue refuses live agents, daemon fixtures and suspended Playwright', () => {
  for (const change of [{ needs_live_agent: true }, { needs_instance: true }, { runner: 'pnpm exec playwright test' }]) {
    const mutant = structuredClone(catalogue);
    Object.assign(mutant.runners.find(entry => entry.id === 'ts-unit'), change);
    assert.ok(ownerErrors(mutant).some(error => error.includes('forbidden PR entry')));
  }
});

