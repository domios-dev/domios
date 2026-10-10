import { at, lineAt } from './records.mjs';
export function summarizeRecords(records, binding, schema) {
  const { provider, sessionId } = binding;
  if (!binding.tabId || !sessionId) throw new Error('Bound tab/session required');
  const turns = [], models = [];
  const add = (role, id, model, line) => {
    if (model && typeof model === 'string') models.push({ model, line });
    if (role === 'user') {
      if (!id) throw new Error(`User turn without provider identity at line ${line}`);
      turns.push({ id, line });
    }
  };
  if (provider === 'codex') {
    const headers = records.filter(row => row.type === 'session_meta');
    if (!headers.length || headers.some(row => row.payload?.id !== sessionId)) throw new Error('Foreign or missing Codex session_meta');
    records.forEach((row, i) => {
      if (row.type === 'turn_context') add(null, null, row.payload?.model, lineAt(records, i));
      if (row.type === 'response_item' && row.payload?.type === 'message' && row.payload.role === 'user') {
        add('user', row.payload.id ?? row.timestamp, null, lineAt(records, i));
      }
    });
  } else if (provider === 'claude') {
    if (!records.some(row => row.sessionId === sessionId)) throw new Error('Missing Claude session');
    records.forEach((row, i) => {
      if (row.sessionId && row.sessionId !== sessionId) throw new Error('Foreign Claude session');
      if (row.sessionId === sessionId && !row.isSidechain) add(row.message?.role ?? row.type, row.uuid, row.message?.model, lineAt(records, i));
    });
  } else if (provider === 'opencode') {
    const sessions = records.filter(row => row.table === 'session');
    if (sessions.length !== 1 || sessions[0].row?.id !== sessionId) throw new Error('Wrong OpenCode session export');
    records.forEach((entry, i) => {
      if (!['message', 'part'].includes(entry.table)) return;
      if (entry.row?.session_id !== sessionId) throw new Error('Foreign OpenCode row');
      if (entry.table === 'message') {
        const data = JSON.parse(entry.row.data);
        add(data.role, entry.row.id, data.modelID ?? data.model?.modelID, lineAt(records, i));
      }
    });
  } else {
    if (!schema?.sessionPath || !schema?.rolePath || !schema?.idPath || !schema?.modelPath) throw new Error('Provider-owned record schema required');
    records.forEach((row, i) => {
      if (at(row, schema.sessionPath) !== sessionId) throw new Error('Unbound provider row');
      add(at(row, schema.rolePath), at(row, schema.idPath), at(row, schema.modelPath), lineAt(records, i));
    });
  }
  if (new Set(turns.map(turn => turn.id)).size !== turns.length) throw new Error('Duplicate provider user-turn identity');
  return { ...binding, turns, models };
}
export function proveMediumChat(summary, expectedModels) {
  if (summary.turns.length < 15 || summary.turns.length > 30) throw new Error('Medium chat requires 15-30 provider-record user turns');
  if (!Array.isArray(expectedModels) || expectedModels.length !== 2 || expectedModels[0] === expectedModels[1]) throw new Error('Two distinct catalogue-approved cheap model ids required');
  const first = summary.models.findIndex(row => row.model === expectedModels[0]);
  const second = summary.models.findIndex((row, i) => i > first && row.model === expectedModels[1]);
  if (first < 0 || second < 0) throw new Error('Model switch missing from own chronological provider records');
  return { turns: summary.turns, switchLines: [summary.models[first].line, summary.models[second].line] };
}
