import { createHash } from 'node:crypto';
import { parseBootPhase, phasesForTab } from './phase-collector.mjs';

export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export function safeUrl(value) {
  const url = new URL(value);
  url.username = ''; url.password = ''; url.search = ''; url.hash = '';
  return url.href;
}

export function assetIdentity(url, bytes, origin) {
  return { url: safeUrl(url), sha256: sha256(bytes), bytes: bytes.length, origin };
}

export function phaseBreakdown({ rows, marks }, state) {
  return rows.map(row => {
    const phases = phasesForTab(marks, row.id);
    const tab = state.tabs?.[row.id];
    const phase = name => phases.find(mark => mark.phase === name)?.atUs ?? null;
    const origins = {
      cli_begin: { value: row.requestBeginAt ?? null, unit: 'epoch_ms', source: 'wrapper Date.now before CLI' },
      socket_received: { value: phase('request_received'), unit: 'epoch_us', source: 'boot-phase request_received' },
      socket_dispatched: { value: phase('request_dispatched'), unit: 'epoch_us', source: 'boot-phase request_dispatched' },
      socket_bound: { value: phase('request_bound'), unit: 'epoch_us', source: 'boot-request bound_at_us' },
      utility_created_at: { value: tab?.utility_created_at || null, unit: 'epoch_us', source: 'exact tab state utility_created_at' },
      PtyStarting: { value: phase('PtyStarting'), unit: 'epoch_us', source: 'boot-phase PtyStarting' },
      AgentReady: { value: phase('AgentReady'), unit: 'epoch_us', source: 'boot-phase AgentReady' },
    };
    const delta = (a, b) => origins[a].value == null || origins[b].value == null ? null
      : (origins[b].value - origins[a].value) / 1000;
    return { tabId: row.id, kind: row.kind ?? null, origins, phases,
      socketQueueMs: delta('socket_received', 'socket_dispatched'),
      creationToReadyMs: delta('utility_created_at', 'AgentReady'),
      missing: Object.entries(origins).filter(([, mark]) => mark.value == null).map(([name]) => name) };
  });
}

export function logMarks(raw, beginUs, endUs) {
  if (!Number.isSafeInteger(beginUs) || !Number.isSafeInteger(endUs) || endUs < beginUs) throw Error('Explicit valid log window required');
  return raw.split('\n').map(parseBootPhase).filter(mark => mark && mark.atUs >= beginUs && mark.atUs <= endUs);
}

export function userIds(chat) {
  const messages = Array.isArray(chat) ? chat : chat.messages ?? chat.data?.messages;
  if (!Array.isArray(messages)) throw Error('Own-session transcript messages absent');
  const ids = messages.filter(message => (message.kind ?? message.type ?? message.role) === 'user').map(message => message.id);
  if (ids.some(id => typeof id !== 'string' || !id) || new Set(ids).size !== ids.length) throw Error('User turn IDs absent or duplicated');
  return ids;
}

export function tabAuthority(tab) {
  if (!tab) return null;
  const { agent_session_id, session_binding_nonce, pty_id, boot_stage } = tab;
  return { agent_session_id, session_binding_nonce, pty_id, boot_stage };
}

export function recoveryVerdict(before, after) {
  if (!before?.tab || !after?.tab || !before.process || !after.process || !Array.isArray(before.userIds) || !before.userIds.length || !Array.isArray(after.userIds))
    return { status: 'incomplete', reason: 'Missing authority, process or own user-turn evidence' };
  const identity = process => Number.isInteger(process.pid) && process.pid > 0 && typeof process.created === 'string' && !!process.created;
  if (!identity(before.process) || !identity(after.process) || !before.tab.agent_session_id || !before.tab.session_binding_nonce || !before.tab.pty_id ||
      !after.tab.session_binding_nonce || !after.tab.pty_id) return { status: 'incomplete', reason: 'Incomplete process or session identity' };
  if ([...before.userIds, ...after.userIds].some(id => typeof id !== 'string' || !id)) return { status: 'incomplete', reason: 'Missing exact user-turn ID' };
  const sameTurns = JSON.stringify(before.userIds) === JSON.stringify(after.userIds) && new Set(after.userIds).size === after.userIds.length;
  const recovered = before.tab.agent_session_id === after.tab.agent_session_id && before.tab.pty_id !== after.tab.pty_id &&
    after.tab.boot_stage === 'agent_ready' &&
    (before.process.pid !== after.process.pid || before.process.created !== after.process.created);
  return { status: recovered && sameTurns ? 'pass' : 'fail', recovered, sameTurns };
}

export function diagnosticVerdict(records) {
  if (!records.length || records.some(record => !record.response)) return { status: 'incomplete' };
  return { status: records.every(record => record.status === 200 && record.response.ok === true && record.body?.stage) ? 'pass' : 'fail' };
}
