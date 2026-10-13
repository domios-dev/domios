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
