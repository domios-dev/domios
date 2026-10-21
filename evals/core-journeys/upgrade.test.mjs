import assert from 'node:assert/strict';
import test from 'node:test';
import { upgradeVerdicts } from './journeys/upgrade.mjs';

function evidence() {
  return {
    nMinusOne: { dataDir: '/data/old', instance: 'fixture', port: 7791 },
    nDataDirAfterFirstStart: '/data/new',
    tabs: [{ id: 'tab-a', session: 'session-a' }, { id: 'tab-b', session: null }],
    planIds: ['plan-a', 'plan-b'],
    memRows: 7,
  };
}

function observation() {
  return {
    dataDir: '/data/new', legacyDirectoryExists: false,
    tabs: [{ id: 'tab-a', agent_session_id: 'session-a' }, { id: 'tab-b', agent_session_id: null }],
    plans: [{ id: 'plan-a' }, { id: 'plan-b' }],
    memRows: 7,
  };
}

test('upgrade oracle passes only after the legacy directory is removed and all recorded data survives', () => {
  const rows = upgradeVerdicts(evidence(), observation());
  assert.deepEqual(rows.map(({ name, pass }) => [name, pass]), [
    ['upgrade_n_data_directory_changed', true],
    ['upgrade_legacy_directory_removed', true],
    ['upgrade_tabs_sessions_preserved', true],
    ['upgrade_plan_ids_preserved', true],
    ['upgrade_mem_row_count_preserved', true],
  ]);
});

test('each data-loss state is a named failure, including a second relocation', () => {
  const expected = evidence();
  const broken = observation();
  broken.legacyDirectoryExists = true;
  broken.tabs = [{ id: 'tab-a', agent_session_id: 'wrong-session' }];
  broken.plans = [{ id: 'plan-a' }];
  broken.memRows = 6;
  broken.dataDir = '/data/old';
  const rows = upgradeVerdicts(expected, broken, { secondStart: true });
  assert.deepEqual(rows.filter((row) => !row.pass).map((row) => row.name), [
    'upgrade_n_data_directory_changed',
    'upgrade_legacy_directory_removed',
    'upgrade_tabs_sessions_preserved',
    'upgrade_plan_ids_preserved',
    'upgrade_mem_row_count_preserved',
    'upgrade_second_start_did_not_relocate',
  ]);
});

test('a missing tab session, plan, or mem row cannot be hidden by other preserved data', () => {
  const broken = observation();
  broken.tabs[0].agent_session_id = null;
  broken.plans.shift();
  broken.memRows += 1;
  const rows = upgradeVerdicts(evidence(), broken);
  assert.equal(rows.find((row) => row.name === 'upgrade_tabs_sessions_preserved').pass, false);
  assert.equal(rows.find((row) => row.name === 'upgrade_plan_ids_preserved').pass, false);
  assert.equal(rows.find((row) => row.name === 'upgrade_mem_row_count_preserved').pass, false);
});
