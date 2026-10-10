import { lineAt } from './records.mjs';
export function verifyGrokPath(originalPath, cwd, sessionId) {
  if (typeof originalPath !== 'string' || !/^(\/|[A-Za-z]:[\\/])/.test(originalPath) || !cwd || !sessionId) throw new Error('Absolute Grok source and bound cwd/session required');
  const parts = originalPath.replaceAll('\\', '/').split('/');
  const [root, sessions, encodedCwd, directorySession, file] = parts.slice(-5);
  if (root !== '.grok' || sessions !== 'sessions' || directorySession !== sessionId || file !== 'updates.jsonl' || decodeURIComponent(encodedCwd) !== cwd) throw new Error('Grok source path does not belong to this tab session');
}
export function summarizeGrok(records, binding) {
  verifyGrokPath(binding.originalPath, binding.cwd, binding.sessionId);
  if (!records.length) throw new Error('Empty Grok session record');
  const turns = [], models = [], prompts = new Map();
  records.forEach((record, i) => {
    const line = lineAt(records, i), params = record.params;
    if (params?.sessionId !== binding.sessionId) throw new Error(`Foreign or missing Grok session at line ${line}`);
    const update = params.update;
    if (!['session/update', '_x.ai/session/update'].includes(record.method) || update?.sessionUpdate !== 'user_message_chunk') return;
    const index = update._meta?.promptIndex, model = update._meta?.modelId;
    if (!Number.isInteger(index) || index < 0) throw new Error(`Missing Grok prompt identity at line ${line}`);
    if (typeof model !== 'string' || !model) throw new Error(`Missing Grok turn model at line ${line}`);
    if (prompts.has(index)) {
      if (prompts.get(index) !== model) throw new Error('Grok prompt chunks disagree on model');
      return;
    }
    if (prompts.size && index <= Math.max(...prompts.keys())) throw new Error('Grok user turns are out of order');
    prompts.set(index, model);
    turns.push({ id: `${binding.sessionId}:prompt:${index}`, line });
    models.push({ model, line });
  });
  return { ...binding, turns, models };
}
