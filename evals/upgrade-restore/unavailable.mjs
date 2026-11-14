import {isDeepStrictEqual} from 'node:util';
import {capturedUnavailable, identityVerdict} from './model.mjs';

export function unavailableVerdict(document, tab, actual, trace) {
  const state = capturedUnavailable(document, tab);
  const identity = identityVerdict(tab, actual);
  let reason = identity?.reason;
  if (!reason && !state) reason = 'capture has no preserved-unavailable state';
  if (!reason && (tab.chatbackend_plugin_id
    ? actual.backing?.kind !== 'chat_backend' || actual.backing.plugin_id !== tab.chatbackend_plugin_id
      || actual.backing.session_id !== tab.chatbackend_session_id
    : actual.backing?.kind !== 'pty')) reason = 'captured unavailable backing changed';
  if (!reason && !isDeepStrictEqual(state, actual.agent_runtime_state)) reason = 'captured unavailable state changed';
  if (!reason && (actual.frozen !== true || actual.pty_id || actual.agent_booting
    || actual.session_binding_nonce !== tab.agent_identity?.session_binding_nonce)) {
    reason = 'unavailable tab was activated';
  }
  const session = tab.claude_session_id ?? tab.agent_session_id;
  if (!reason && trace.some(row => row.thread_id === session)) reason = 'unavailable session reached provider runtime';
  return {tab_id: tab.tab_id, bucket: 'preserved-unavailable', preserved: !reason,
    reason: reason ?? 'captured unavailable state and identity preserved without activation'};
}
