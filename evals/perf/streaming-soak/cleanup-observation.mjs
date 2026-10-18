export function cleanupStateComplete(state,tabs,workspace) {
  if (!state || !state.tabs || typeof state.tabs !== 'object' || Array.isArray(state.tabs) || !Array.isArray(state.workspaces) ||
      !Array.isArray(tabs) || tabs.length === 0 || typeof workspace !== 'string' || !workspace) return false;
  return tabs.every(tab => typeof tab === 'string' && !Object.hasOwn(state.tabs,tab)) &&
    state.workspaces.every(item => item?.id !== workspace);
}

export async function installCleanupObserverPage({tabs,workspace,maxEvents=64,deadlineMs=20000},isComplete,loadModule=specifier => import(specifier)) {
  if (window.__streamingSoakCleanup) throw Error('Cleanup observation already installed');
  if (!Array.isArray(tabs) || tabs.length === 0 || tabs.some(tab => typeof tab !== 'string' || !tab) || new Set(tabs).size !== tabs.length ||
      typeof workspace !== 'string' || !workspace || typeof isComplete !== 'function' ||
      !Number.isSafeInteger(maxEvents) || maxEvents < 1 || maxEvents > 64 || !Number.isSafeInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > 20000) throw Error('Invalid owned cleanup observation');
  const bridge = await loadModule('/src/lib/bridge.ts');
  if (typeof bridge.listen !== 'function' || typeof bridge.invoke !== 'function') throw Error('Cleanup event bridge unavailable');
  const deadline = AbortSignal.timeout(deadlineMs);
  let resolveWait, rejectWait, resolveReady, rejectReady, unlisten = null, settled = false, draining = false, queued = 0, events = 0, reads = 0;
  const completion = new Promise((resolve,reject) => { resolveWait = resolve; rejectWait = reject; });
  const ready = new Promise((resolve,reject) => { resolveReady = resolve; rejectReady = reject; });
  completion.catch(() => {});
  const finish = (error,receipt) => {
    if (settled) return;
    settled = true;
    deadline.removeEventListener('abort',onDeadline);
    const stop = unlisten; unlisten = null;
    try { stop?.(); } catch {}
    if (error) rejectWait(error); else resolveWait(receipt);
  };
  const onDeadline = () => {
    const error = Error('Owned mux cleanup observation deadline');
    rejectReady(error);
    finish(error);
  };
  deadline.addEventListener('abort',onDeadline,{once:true});
  const drain = async () => {
    if (draining || settled) return;
    draining = true;
    try {
      while (queued > 0 && !settled) {
        queued--; reads++;
        let state;
        try { state = await bridge.invoke('mux_get_state'); }
        catch { finish(Error('Owned mux cleanup state read failed')); return; }
        if (isComplete(state,tabs,workspace)) finish(null,{events,reads,tabsAbsent:true,workspaceAbsent:true});
      }
    } finally {
      draining = false;
      if (queued > 0 && !settled) void drain();
    }
  };
  const onChange = () => {
    if (settled) return;
    if (++events > maxEvents) { finish(Error('Owned mux cleanup event budget exceeded')); return; }
    queued++;
    void drain();
  };
  try {
    Promise.resolve().then(() => bridge.listen('mux-state-changed',onChange)).then(stop => {
      if (settled) { try { stop?.(); } catch {} }
      else unlisten = stop;
      resolveReady();
    },() => {
      const error = Error('Owned mux cleanup listener unavailable');
      rejectReady(error);
      finish(error);
    });
    await ready;
  } catch (error) {
    const failure = error?.message === 'Owned mux cleanup observation deadline'
      ? error : Error('Owned mux cleanup listener unavailable');
    finish(failure);
    throw failure;
  }
  window.__streamingSoakCleanup = {
    wait:() => completion,
    dispose:() => {
      finish(Error('Owned mux cleanup observation disposed'));
      delete window.__streamingSoakCleanup;
    },
  };
  return {ready:true};
}
