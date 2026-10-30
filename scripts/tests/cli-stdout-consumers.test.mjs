import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const python = process.platform === 'win32' ? 'python' : 'python3';

test('orchestration expressions unwrap CLI results and keep raw API input explicit', () => {
  const cases = [
    [{ result: [{ id: 'one' }, { id: 'two' }], meta: { total: 2 } }, 'len(d)', [], '2'],
    [{ result: { plan_id: 'plan' } }, "d.get('plan_id')", [], 'plan'],
    [{ tabs: { one: {} } }, "len(d['tabs'])", ['--raw'], '1'],
  ];
  for (const [input, expression, flags, expected] of cases) {
    const result = spawnSync(python, ['evals/orchestrator-creation-matrix/json_expr.py', expression, ...flags], {
      input: JSON.stringify(input), encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), expected);
  }
});

test('lifecycle observations use CLI result arrays and records without changing API snapshots', () => {
  for (const [mode, args, input, output] of [
    ['team-count', ['team', '1'], { result: [{ id: 'team' }] }, ''],
    ['plan-state', ['plan', 'team', 'owner'], { result: { id: 'plan', team_id: 'team', status: 'active', owner: { tab_id: 'owner' } } }, ''],
    ['field', ['.result.tab_id'], { result: { tab_id: 'created' } }, 'created'],
    ['general-agent', [], [{ id: 'general', role_profile: 'general_agent' }], 'general'],
  ]) {
    const result = spawnSync(process.execPath, ['evals/lifecycle-recovery/json-state.mjs', mode, ...args], {
      input: JSON.stringify(input), encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), output);
  }
});
