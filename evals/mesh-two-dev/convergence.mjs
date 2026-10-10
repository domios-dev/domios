function indexTabs(tabs) {
  const map = new Map();
  for (const tab of tabs) {
    if (!tab.id || !tab.sessionId || !tab.provider || map.has(tab.id)) throw new Error('Missing binding or ghost duplicate tab');
    map.set(tab.id, tab);
  }
  return map;
}
export function verifyConvergence(before, after, closed = []) {
  const baseline = indexTabs(before.tabs), current = indexTabs(after.tabs);
  const removed = new Set(closed);
  for (const id of removed) if (!baseline.has(id)) throw new Error('Unknown closed tab');
  if (current.size !== baseline.size - removed.size) throw new Error('Ghost or lost tab');
  for (const [id, old] of baseline) {
    if (removed.has(id)) {
      if (current.has(id)) throw new Error('Stale closed tab');
      continue;
    }
    const next = current.get(id);
    if (!next || next.sessionId !== old.sessionId || next.provider !== old.provider) throw new Error(`Lost/rebound tab: ${id}`);
    if (!Array.isArray(old.turnIds) || !Array.isArray(next.turnIds) || new Set(old.turnIds).size !== old.turnIds.length) throw new Error('Turn identities required');
    if (new Set(next.turnIds).size !== next.turnIds.length || old.turnIds.some((turn, i) => turn !== next.turnIds[i])) throw new Error(`Lost, duplicated or reordered history: ${id}`);
  }
  return true;
}
export function verifyRestart({ instance, oldBoot, newBoot, lifecycle, views, baseline, closed = [] }) {
  if (!['aiderqa', 'meshqa'].includes(instance) || !oldBoot || !newBoot || oldBoot === newBoot) throw new Error('Named DEV restart generation required');
  if (!lifecycle?.listenerReady || !lifecycle?.reconnected || lifecycle.boot !== newBoot) throw new Error('Restart readiness events missing');
  for (const view of ['local', 'remote', 'web']) {
    if (!views[view] || views[view].boot !== newBoot || views[view].reconciled !== true) throw new Error(`View not reconverged: ${view}`);
    verifyConvergence(baseline, views[view], closed);
  }
  const histories = ['local', 'remote', 'web'].map(name => JSON.stringify(views[name].tabs.map(({ id, sessionId, provider, turnIds }) => ({ id, sessionId, provider, turnIds })).sort((a, b) => a.id.localeCompare(b.id))));
  if (new Set(histories).size !== 1) throw new Error('Views disagree after restart');
  return true;
}
export async function coordinateRestart(adapter, instance, signal, write) {
  if (!['aiderqa', 'meshqa'].includes(instance)) throw new Error('Only named QA DEVs may restart');
  const bounded = AbortSignal.any([signal, AbortSignal.timeout(120000)]);
  let before;
  try {
    bounded.throwIfAborted();
    before = await adapter.snapshot(instance, bounded);
    await write({ kind: 'before-restart', instance, before });
    const authorization = await adapter.authorizedUtility(instance, bounded);
    if (authorization.instance !== instance || authorization.port !== (instance === 'aiderqa' ? 7697 : 7698) || authorization.canonical !== true || !authorization.exactGate) throw new Error('Exact canonical named-DEV utility authorization required');
    await adapter.restart(authorization, bounded);
    const evidence = await adapter.waitReconverged(instance, bounded);
    await write({ kind: 'after-restart', instance, evidence });
    return verifyRestart({ instance, baseline: before, ...evidence });
  } catch (error) {
    await write({ kind: 'restart-failure', instance, before, error: error.message, status: 'FAIL' });
    throw error;
  }
}
