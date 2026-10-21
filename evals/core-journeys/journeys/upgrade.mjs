import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dcJson } from '../lib/env.mjs';
import { readTabs } from '../lib/tabs.mjs';

function idOf(row) { return row?.id ?? row?.plan_id ?? null; }

export function upgradeVerdicts(baseline, observed, { secondStart = false } = {}) {
  const expectedTabs = Array.isArray(baseline.tabs) ? baseline.tabs : [];
  const actualTabs = new Map((observed.tabs ?? []).map((tab) => [tab.id, tab]));
  const tabsMatch = expectedTabs.length > 0 && expectedTabs.every((expected) => {
    const actual = actualTabs.get(expected.id);
    return actual && (actual.agent_session_id ?? actual.session ?? null) === expected.session;
  });
  const actualPlanIds = new Set((observed.plans ?? []).map(idOf).filter(Boolean));
  const plansMatch = baseline.planIds.length > 0 && baseline.planIds.every((id) => actualPlanIds.has(id));
  const memoryMatch = Number.isInteger(baseline.memRows) && Number.isInteger(observed.memRows)
    && baseline.memRows === observed.memRows;
  const rows = [
    { name: 'upgrade_n_data_directory_changed', pass: Boolean(observed.dataDir) && observed.dataDir !== baseline.nMinusOne.dataDir,
      expected: baseline.nMinusOne.dataDir, actual: observed.dataDir },
    { name: 'upgrade_legacy_directory_removed', pass: observed.legacyDirectoryExists === false,
      expected: false, actual: observed.legacyDirectoryExists },
    { name: 'upgrade_tabs_sessions_preserved', pass: tabsMatch,
      expected: expectedTabs, actual: observed.tabs ?? [] },
    { name: 'upgrade_plan_ids_preserved', pass: plansMatch,
      expected: baseline.planIds, actual: [...actualPlanIds] },
    { name: 'upgrade_mem_row_count_preserved', pass: memoryMatch,
      expected: baseline.memRows, actual: observed.memRows },
  ];
  if (secondStart) rows.push({
    name: 'upgrade_second_start_did_not_relocate',
    pass: Boolean(baseline.nDataDirAfterFirstStart) && observed.dataDir === baseline.nDataDirAfterFirstStart
      && observed.legacyDirectoryExists === false && tabsMatch && plansMatch && memoryMatch,
    expected: baseline.nDataDirAfterFirstStart,
    actual: observed.dataDir,
  });
  return rows;
}

function connectionOf(identity) {
  const connection = identity?.connection;
  if (!connection?.data_dir || !connection?.instance || !Number.isInteger(Number(connection.port))) {
    throw new Error(`CLI identify returned no named connection identity: ${JSON.stringify(identity).slice(0, 500)}`);
  }
  return { dataDir: connection.data_dir, instance: connection.instance, port: Number(connection.port) };
}

function memRows(stats) {
  const count = stats?.total_items ?? stats?.stats?.total_items;
  if (!Number.isInteger(count) || count < 0) throw new Error(`mem stats has no total_items count: ${JSON.stringify(stats).slice(0, 500)}`);
  return count;
}

export async function captureUpgradeBaseline(ctx, outputPath) {
  const identity = connectionOf(await dcJson(['identify']));
  const [tabs, stats] = await Promise.all([readTabs(ctx.api), dcJson(['mem', 'stats'])]);
  const expectedIds = new Set((ctx.state.tabs ?? []).map((tab) => tab.id));
  const rows = tabs.filter((tab) => expectedIds.has(tab.id));
  const planIds = [...new Set((ctx.state.plans ?? []).filter((id) => typeof id === 'string' && id.length > 0))];
  if (!rows.length || !planIds.length) throw new Error('N-1 setup did not produce tab/session and plan evidence');
  const baseline = {
    version: 1,
    nMinusOne: { ...identity, cliName: process.env.CORE_JOURNEY_CLI_NAME ?? null },
    tabs: rows.map((tab) => ({ id: tab.id, session: tab.agent_session_id ?? null })),
    planIds,
    memRows: memRows(stats),
  };
  writeFileSync(outputPath, `${JSON.stringify(baseline, null, 2)}\n`);
  ctx.state.upgrade_baseline = { path: outputPath, tabCount: rows.length, planCount: planIds.length, memRows: baseline.memRows };
  ctx.save();
  return baseline;
}

export async function verifyUpgrade(ctx, baselinePath, { secondStart = false } = {}) {
  const baseline = JSON.parse(readFileSync(baselinePath, 'utf8'));
  if (baseline.version !== 1 || !baseline.nMinusOne?.dataDir || !Array.isArray(baseline.tabs) || !Array.isArray(baseline.planIds)) {
    throw new Error('N-1 upgrade baseline is incomplete');
  }
  const identity = connectionOf(await dcJson(['identify']));
  const [tabs, plans, stats] = await Promise.all([
    readTabs(ctx.api), ctx.api('/api/plans?full=1'), dcJson(['mem', 'stats']),
  ]);
  const planRows = Array.isArray(plans) ? plans : plans?.plans ?? [];
  const observed = {
    ...identity,
    legacyDirectoryExists: existsSync(baseline.nMinusOne.dataDir),
    tabs: tabs.map((tab) => ({ id: tab.id, agent_session_id: tab.agent_session_id ?? null })),
    plans: planRows,
    memRows: memRows(stats),
  };
  const rows = upgradeVerdicts(baseline, observed, { secondStart });
  for (const row of rows) {
    await ctx.check('J5', row.name, baseline.nMinusOne.instance, async () => {
      if (!row.pass) throw Object.assign(new Error(`expected ${JSON.stringify(row.expected)}; got ${JSON.stringify(row.actual)}`), { evidence: row });
      return { summary: JSON.stringify({ expected: row.expected, actual: row.actual }), evidence: row };
    });
  }
  if (!secondStart) {
    baseline.nDataDirAfterFirstStart = identity.dataDir;
    writeFileSync(baselinePath, `${JSON.stringify(baseline, null, 2)}\n`);
  }
  return rows;
}
