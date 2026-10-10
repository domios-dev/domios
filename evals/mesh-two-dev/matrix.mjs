import { targetNames } from './targets.mjs';
export const providers = ['claude', 'codex', 'opencode', 'grok'];
export const directions = targetNames.map(name => `${name}->${targetNames.find(other => other !== name)}`);
export const views = ['local', 'remote', 'web'];
export const requirements = {
  1: ['working', 'done', 'sending-dot', 'elapsed', 'progress', 'parity-1000ms'],
  3: ['15-30-turns', 'tools', 'code-blocks', 'attachments', 'stop', 'model-switch', 'reload-history'],
  4: ['ordered-events', 'restart-aiderqa', 'restart-windows', 'reconverged', 'no-ghosts', 'remote-close'],
};
export const meshOps = ['pair-only', 'tab-list', 'send', 'get-text', 'agent-report', 'paste-roundtrip'];
export function matrix(extraProviders = []) {
  const rows = [];
  for (const direction of directions) {
    for (const provider of [...providers, ...extraProviders]) {
      for (const check of [1, 3, 4]) {
        for (const view of views) rows.push({ direction, provider, check, view, status: 'NOT_RUN' });
      }
    }
    for (const check of meshOps) rows.push({ direction, provider: 'mesh', check, view: 'control', status: 'NOT_RUN' });
  }
  return rows;
}
export function key(row) {
  return JSON.stringify([row.direction, row.provider, row.check, row.view]);
}
export function applyVerdict(rows, verdict) {
  const row = rows.find(item => key(item) === key(verdict));
  if (!row) throw new Error('Unknown matrix row');
  if (!['PASS', 'FAIL'].includes(verdict.status)) throw new Error('Invalid verdict');
  if (!Array.isArray(verdict.evidence) || !verdict.evidence.length) throw new Error('Evidence required');
  if (verdict.status === 'FAIL') {
    const repro = verdict.repro;
    if (!repro?.steps?.length || !repro.tabId || !repro.sessionId || !repro.logLines?.length) {
      throw new Error('Exact steps, tab/session ids and log lines required');
    }
  }
  if (verdict.status === 'PASS') {
    for (const condition of requirements[verdict.check] ?? [verdict.check]) {
      if (verdict.observations?.[condition] !== true) throw new Error(`Missing observation: ${condition}`);
    }
    if (verdict.check === 3 && !verdict.providerProof) throw new Error('Provider-owned record proof required');
  }
  if (row.status === 'FAIL') throw new Error('Preserve FAIL; start a new run for retest');
  Object.assign(row, verdict);
  return rows;
}
export function markdown(rows) {
  return '| Direction | Provider | Check | View | Status | Evidence |\n|---|---|---|---|---|---|\n' + rows.map(row =>
    `| ${row.direction} | ${row.provider} | ${row.check} | ${row.view} | ${row.status} | ${(row.evidence ?? []).join(', ')} |`
  ).join('\n') + '\n';
}
