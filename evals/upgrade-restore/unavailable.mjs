import {isDeepStrictEqual} from 'node:util';
import {capturedFailedLaunch, capturedUnavailable, identityVerdict} from './model.mjs';

export function failedLaunchVerdict(document, tab, actual, trace) {
  const evidence = capturedFailedLaunch(document, tab);
  let reason = actual ? null : 'tab missing after restore';
  for (const key of ['agent_type', 'role', 'role_profile', 'team_id', 'parent_tab_id']) {
    if (!reason && (tab[key] ?? null) !== (actual[key] ?? null)) reason = `${key} changed after restore`;
  }
  if (!reason && !evidence) reason = 'capture has no typed failed-launch evidence';
  if (!reason && (actual.boot_stage !== 'spawn_failed'
    || (actual.boot_failure_reason ?? null) !== (evidence.boot_failure_reason ?? null))) {
    reason = 'captured launch failure changed';
  }
  if (!reason && (actual.backing?.kind !== 'pty' || actual.frozen !== true
    || actual.pty_id || actual.agent_booting || actual.reported_session_binding_nonce
    || actual.agent_state || actual.agent_runtime_state
    || (actual.agent_session_id ?? null) !== (tab.claude_session_id ?? tab.agent_session_id ?? null))) reason = 'failed launch was activated';
  const expectedIdentity = tab.agent_identity ?? {};
  for (const key of ['role_profile', 'team_id', 'parent_tab_id', 'can_spawn', 'depth', 'shell_profile']) {
    if (!reason && !isDeepStrictEqual(actual[key] ?? null, tab[key] ?? null)) reason = `${key} changed after restore`;
  }
  for (const [key, value] of Object.entries({
    session_binding_nonce: expectedIdentity.session_binding_nonce,
    task_binding: expectedIdentity.task_binding,
    lifecycle_epoch: expectedIdentity.lifecycle_epoch,
    lifecycle_operation_id: expectedIdentity.lifecycle_operation_id,
  })) {
    if (!reason && !isDeepStrictEqual(actual[key] ?? null, value ?? null)) reason = `${key} changed after restore`;
  }
  const account = expectedIdentity.restorable_launch;
  if (!reason && account?.account_id != null && actual.account?.id !== account.account_id) reason = 'account changed after restore';
  if (!reason && account?.account_namespace != null && actual.account?.namespace !== account.account_namespace) reason = 'account namespace changed after restore';
  const session = tab.claude_session_id ?? tab.agent_session_id;
  if (!reason && trace.some(row => row.tab_id === tab.tab_id || (session && (row.thread_id === session || row.session_id === session)))) {
    reason = 'failed launch reached provider runtime';
  }
  return {tab_id: tab.tab_id, bucket: 'preserved-failed-launch', preserved: !reason,
    reason: reason ?? 'captured launch failure and hierarchy preserved without activation'};
}

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
