import {isDeepStrictEqual} from 'node:util';

export const VERSION = 1;
export const SAMPLE_MS = 60_000;
export function requireValue(ok, reason) { if (!ok) throw Error(reason); }
export function segmentSeconds(value) {
  requireValue(Number.isInteger(value) && value > 0 && value <= 480, 'Segment must be 1..480 seconds');
  return value;
}
export function validateIdentity(identity) {
  requireValue(identity?.port === 7686 && identity.dev === true, 'Shared DEV only; PROD refused');
  requireValue(/^[a-f0-9]{40}$/.test(identity.build ?? ''), 'Exact DEV SHA required');
  requireValue(/^[a-f0-9]{64}$/.test(identity.entryHash ?? ''), 'Served entry digest required');
  for (const key of ['renderer', 'daemon']) {
    requireValue(Number.isSafeInteger(identity[key]?.pid) && identity[key].pid > 0 &&
      typeof identity[key].created === 'string' && identity[key].created.length > 0, `Missing ${key} PID/creation`);
  }
  requireValue(Boolean(identity.target && identity.boot), 'Missing page/boot identity');
  return identity;
}
export function muxBindings(state) {
  const membership = new Map();
  for (const workspace of state.workspaces) for (const tab of workspace.tabs) {
    requireValue(!membership.has(tab),'Duplicate workspace membership'); membership.set(tab,workspace.id);
  }
  return Object.fromEntries(Object.entries(state.tabs).map(([id,tab]) => [id,{...tab,workspace_id:membership.get(id)??null}]));
}
export function validateResume(cp, actual, bindings) {
  requireValue(cp?.version === VERSION && cp.stage === 'ready', 'Checkpoint is not resumable');
  validateIdentity(cp.identity); validateIdentity(actual);
  requireValue(isDeepStrictEqual(cp.identity, actual), 'DEV identity drift');
  requireValue(Number.isSafeInteger(cp.batch) && cp.batch >= 0 && cp.pending === null, 'Ambiguous unfinished batch; never replay');
  requireValue(typeof cp.run === 'string' && /^[a-f0-9-]{36}$/.test(cp.run), 'Invalid owner');
  requireValue(Array.isArray(cp.fixtures) && cp.fixtures.length === 1 && cp.terminal?.tab, 'Missing owned fixtures');
  const owned = [...cp.fixtures, cp.terminal];
  requireValue(new Set(owned.map(f => f.tab)).size === owned.length, 'Duplicate owned tabs');
  for (const f of owned) {
    const live = bindings[f.tab];
    requireValue(typeof f.pty === 'string' && f.pty.length > 0 && typeof f.tab === 'string' && /^[a-zA-Z0-9_-]+$/.test(f.tab) &&
      f.run === cp.run && live?.workspace_id === cp.workspace.id &&
      (live.agent_session_id ?? null) === f.session && live.pty_id === f.pty, 'Owned tab/session/PTY drift');
  }
  return cp;
}
export function cleanupPlan(cp, actual, bindings) {
  validateResume({...cp, stage:'ready'}, actual, bindings);
  requireValue(cp.pending === null, 'Unfinished batch');
  const tabs = [...cp.fixtures.map(f => f.tab), cp.terminal.tab];
  requireValue(Object.entries(bindings).every(([id,tab]) => tab.workspace_id !== cp.workspace.id || tabs.includes(id)), 'Foreign tab in owned workspace');
  return tabs;
}
export function validateFileOwnership(cp,observed) {
  requireValue(observed.owner === cp.run && observed.history === cp.history && observed.junctionMatches && observed.inventoryMatches,'Owned fixture directory drift');
  requireValue(cp.fixtures.every(f => observed.hashes[f.path] === f.hash),'Fixture contents drift');
}
export function emitterDecision(state, message) {
  requireValue(message?.version === VERSION && message.run === state.run && message.token === state.token, 'Foreign emitter request');
  if (message.op === 'status') return {op:'status', batch:state.batch};
  requireValue(!state.stopped, 'Emitter stopped');
  if (message.op === 'stop') return {op:'stop', batch:state.batch};
  requireValue(message.op === 'batch' && Number.isSafeInteger(message.batch) && message.batch === state.batch + 1, 'Batch must be exact next number');
  requireValue(Number.isInteger(message.lines) && message.lines >= 1 && message.lines <= 128, 'Batch lines must be 1..128');
  return {op:'batch', batch:message.batch, lines:message.lines};
}
export function sampleDue(now, previous) { return previous === null || now - previous >= SAMPLE_MS; }
export function acknowledge(expected, reply) {
  requireValue(reply?.batch === expected && reply.ok === true, 'Exact batch acknowledgement missing');
}
