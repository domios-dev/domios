import {isDeepStrictEqual} from 'node:util';

export function utilityKind(tab) {
  if (tab.agent_type) return null;
  if (tab.chatbackend_plugin_id) throw new Error(`tab=${tab.tab_id}: fake chat backend missing`);
  return tab.surface ? 'surface' : 'pty';
}

export function requireUtility(tab, history) {
  const kind = utilityKind(tab);
  if (!kind) return;
  if (history.shape.length || history.transcript?.status !== 'not_applicable'
    || history.transcript.backing_kind !== kind || history.transcript.provider != null
    || tab.agent_session_id || tab.claude_session_id || tab.role || tab.role_profile
    || tab.agent_identity?.task_binding || tab.agent_identity?.agent_runtime_config) {
    throw new Error(`tab=${tab.tab_id}: unsupported utility backing evidence`);
  }
}

export function utilityVerdict(tab, actual) {
  const fail = reason => ({tab_id: tab.tab_id, ready: false, reason});
  const kind = utilityKind(tab);
  for (const key of ['cwd', 'title', 'purpose']) {
    if ((tab[key] ?? (key === 'purpose' ? 'shell' : null)) !== (actual[key] ?? null)) {
      return fail(`${key} changed after restore`);
    }
  }
  if (actual.agent_session_id || actual.backing?.kind !== kind) return fail('utility backing changed after restore');
  if (kind === 'surface') {
    const {kind: ignored, ...surface} = actual.backing;
    if (actual.pty_id || !isDeepStrictEqual(surface, tab.surface)) return fail('surface identity changed after restore');
    return {tab_id: tab.tab_id, ready: true, reason: 'fixture surface backing restored without a PTY'};
  }
  if (!isDeepStrictEqual(actual.shell_profile, tab.shell_profile)) return fail('shell profile changed after restore');
  if (!actual.pty_id || actual.frozen !== false || actual.boot_stage !== 'shell_ready') {
    return fail('utility shell has no ready live PTY');
  }
  return {tab_id: tab.tab_id, ready: true, reason: 'utility shell restored with ready PTY and captured identity'};
}
