export function unwrap(value) {
  return value.data || value;
}

export function taskStatus(value) {
  const payload = unwrap(value);
  return (payload.task || payload).status;
}

export function restoredOnNewBuild(fixture, health, tab) {
  return health.build_sha !== fixture.build
    && tab.agent_session_id === fixture.session
    && tab.backing?.kind === 'chat_backend'
    && tab.agent_runtime_state?.state === 'loaded';
}

export function isCompactionItem(item) {
  return item.type === 'system'
    && (item.systemSubtype === 'contextCompaction' || item.timelineMarker?.kind === 'context_compaction');
}

export async function completedAssistantReply({ history, send, waitForReply, waitIdle }) {
  const before = new Set((await history()).map(message => message.id));
  await send();
  await waitForReply(messages => messages.some(message => message.type === 'assistant' && !before.has(message.id)));
  await waitIdle();
  return (await history())
    .filter(message => message.type === 'assistant' && !before.has(message.id))
    .map(message => message.content).join('\n');
}

export class NotApplicable extends Error {
  constructor(evidence) {
    super(JSON.stringify(evidence));
    this.evidence = evidence;
  }
}

export async function captureOutcome(action) {
  try {
    return { status: 'PASS', evidence: await action() };
  } catch (error) {
    return error instanceof NotApplicable
      ? { status: 'N/A', evidence: error.evidence }
      : { status: 'FAIL', evidence: error.message };
  }
}

export function turnApproval(tab, status, target, marker) {
  const prompt = unwrap(status).pending_prompt;
  const paths = [target, JSON.stringify(target).slice(1, -1)];
  if (!prompt?.request_id || !Array.isArray(prompt.options)
    || !paths.some(path => prompt.question?.includes(path))
    || !prompt.question?.includes(marker)) return null;
  let token;
  try { token = JSON.parse(prompt.request_id); } catch { return null; }
  if (!Array.isArray(token) || token.length !== 3
    || token[0] !== tab.agent_runtime_state?.epoch
    || token[1] !== tab.session_binding_nonce
    || tab.agent_runtime_state?.state !== 'loaded'
    || !tab.agent_session_id || tab.backing?.kind !== 'chat_backend') return null;
  const index = prompt.options.findIndex(option => /^(approve|allow once|grant for turn)$/i.test(option.label));
  return index < 0 ? null : { prompt, option: prompt.options[index], index: index + 1 };
}
