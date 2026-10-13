const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');

const fixtures = JSON.parse(fs.readFileSync(path.join(__dirname, 'shared.json'), 'utf8'));
const source = fs.readFileSync(path.join(__dirname, '../../rotterdom.rs'), 'utf8');
const setup = source.match(/let setup = r#"([\s\S]*?)"#/)[1]
  .replace('FAILURE', '""')
  .replace('FIXTURES', JSON.stringify(fixtures));
const bundle = fs.readFileSync(path.join(__dirname, 'plugin.js'), 'utf8');

function model() {
  const context = { host: { fetch: {} }, module: { exports: {} }, exports: {} };
  vm.runInNewContext(`${setup}\n${bundle}`, context);
  const plugin = context.module.exports.default;
  plugin.transportErrors = [];
  const fetch = context.host.fetch.async;
  context.host.fetch.async = (...args) => {
    try { return fetch(...args); }
    catch (error) { plugin.transportErrors.push(error.message); throw error; }
  };
  return plugin;
}

function sorted(value) {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, sorted(value[key])]));
  }
  return value;
}

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

test('pinned parity transport accepts Rust-sorted primitive requests', () => {
  for (const fixture of fixtures) {
    const plugin = model();
    assert.deepEqual(plain(plugin.decide(sorted(fixture.request), 2000)), fixture.answer, plugin.transportErrors.join('; '));
  }
});

test('pinned parity transport accepts Rust-sorted named questions and usage', () => {
  const questions = Object.fromEntries(fixtures.map(({ request }) => [
    request.question.type, { instructions: request.instructions, question: request.question },
  ]));
  const answers = Object.fromEntries(fixtures.map(({ request, answer }) => [request.question.type, answer]));
  assert.deepEqual(plain(model().decideQuestions(sorted({ state: { fixture: 'shared' }, questions }), 2000)), {
    answers, usage: { input_tokens: 123, output_tokens: 45 },
  });
});

for (const [name, index, mutate, error] of [
  ['instructions', 0, request => { request.instructions = 'changed'; }, 'question changed'],
  ['criteria', 0, request => { request.question.criteria.true = 'changed'; }, 'question changed'],
  ['state', 0, request => { request.state.fixture = 'changed'; }, 'state changed'],
  ['level order', 2, request => { request.question.levels.reverse(); }, 'question changed'],
]) {
  test(`pinned parity transport rejects changed ${name}`, () => {
    const request = sorted(fixtures[index].request);
    mutate(request);
    const plugin = model();
    assert.deepEqual(plain(plugin.decide(request, 2000)), { error: 'transport' });
    assert.deepEqual(plugin.transportErrors, [error]);
  });
}
