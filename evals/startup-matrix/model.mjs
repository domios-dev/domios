import { kind, textContent } from '../restart-endurance/oracle.mjs';

export const WINDOW_MS = 2 * 24 * 60 * 60 * 1000;
export const remainingDeadline = (now, startedAt, limit) => Math.max(1, Math.ceil(limit - (now - startedAt)));
export function replyWindow(messages, prompt) {
  const index = messages.findLastIndex(m => kind(m) === 'user' && textContent(m.content).includes(prompt));
  if (index < 0) return null;
  const following = messages.slice(index + 1);
  const nextUser = following.findIndex(m => kind(m) === 'user');
  return { user: messages[index], following: nextUser < 0 ? following : following.slice(0, nextUser) };
}
export function policy(tab, workspace, now) {
  return workspace?.name === 'General' || (tab.last_active_at_ms != null && now - tab.last_active_at_ms <= WINDOW_MS)
    ? 'awake' : 'asleep';
}
export function inventory(state, now) {
  return Object.values(state.tabs).filter(t => t.agent_type && !t.hidden).map(tab => {
    const workspace = state.workspaces.find(w => w.tabs.includes(tab.id));
    return { id: tab.id, title: tab.title, provider: tab.agent_type, workspace: workspace?.name,
      workspace_id: workspace?.id, policy: policy(tab, workspace, now), last_active_at_ms: tab.last_active_at_ms,
      frozen: tab.frozen, boot_stage: tab.boot_stage, backing: tab.backing?.kind, session: tab.agent_session_id };
  });
}
export function matrix(rows) {
  const chosen = rows.filter(t => t.workspace === 'General' || t.workspace === 'Endure');
  for (const t of rows.filter(t => t.policy === 'awake' && !chosen.some(c => c.id === t.id)).slice(0, 1))
    if (!chosen.some(c => c.id === t.id)) chosen.push(t);
  if (!chosen.some(t => t.policy === 'asleep')) {
    const frozen = rows.find(t => t.policy === 'asleep');
    if (frozen) chosen.push(frozen);
  }
  return chosen;
}
export function evaluateSla(tab) {
  const chat = tab.backing === 'chat_backend';
  const ms = chat ? tab.accepting_ms : tab.boot?.frozen ? tab.wake_ms : tab.ready_ms;
  const limit = chat ? 1000 : 5000;
  return { surface: chat ? 'chat' : 'pty', limit_ms: limit, measured_ms: ms ?? null,
    pass: ms != null && ms <= limit && tab.reply_ok && !(tab.send?.code > 0 && !tab.send.retained) };
}
export function lockSites(lines) {
  const groups = new Map();
  for (const line of lines) {
    const hold = line.match(/\[LOCK-HOLD\]\s+(.*?) held (\d+)ms(?: \(waited \d+ms\))?(?: at (.*))?/);
    const slow = line.includes('[SLOW-IPC]');
    if (!slow && (!hold || Number(hold[2]) <= 500)) continue;
    const key = hold ? `${hold[1]} at ${hold[3] ?? 'unknown'}` : line.match(/\[SLOW-IPC\]\s+(\S+)/)?.[1] ?? '[SLOW-IPC] unknown';
    const g = groups.get(key) ?? { site: key, kind: hold ? 'lock' : 'ipc', count: 0, total_ms: 0, max_ms: 0, lines: [] };
    const ms = hold ? Number(hold[2]) : Number(line.match(/\[SLOW-IPC\].*?(\d+(?:\.\d+)?)\s*ms/)?.[1] ?? 0);
    g.count++; g.total_ms += ms; g.max_ms = Math.max(g.max_ms, ms); g.lines.push(line); groups.set(key, g);
  }
  return [...groups.values()].sort((a, b) => b.total_ms - a.total_ms);
}
const cell = x => String(x ?? '—').replaceAll('|', '\\|').replaceAll('\n', ' ');
export function render(result) {
  return `# Startup matrix\n\nBuild: ${result.build}. Restart: ${result.restart?.verified ?? false}.\n\n` +
    '| tab | provider | policy | observed | ready ms | reply ok | reply ms | SLA | notes |\n|---|---|---|---|---:|---|---:|---|---|\n' +
    result.tabs.map(t => `| ${[`${t.title} (${t.id})`, t.provider, t.policy, t.observed, t.ready_ms ?? t.wake_ms, t.reply_ok, t.reply_ms,
      t.sla ? `${t.sla.pass ? 'PASS' : 'FAIL'} ${t.sla.measured_ms ?? 'unproven'}/${t.sla.limit_ms} ms` : 'not measured', t.notes.join('; ')].map(cell).join(' | ')} |`).join('\n') +
    `\n\nCoverage: ${result.coverage.join('; ')}\n\nRun failures: ${result.failures.map(cell).join('; ')}\n\nTiming: ready_ms is SSE receipt time minus the new listener's kernel start time. Replayed events give an upper bound; replay gaps are retained.\n`;
}
