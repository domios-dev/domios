export interface PreviewPreparation {
  state: "ready" | "indexing" | "failed";
  index_id: string;
}

interface Ports {
  prepare(tabId: string): Promise<PreviewPreparation>;
  listen(tabId: string, handler: (state: PreviewPreparation) => void): Promise<() => void>;
  reconnect?(handler: () => void): () => void;
}

export function createPreviewPreparation(ports: Ports) {
  const pending = new Map<string, Promise<void>>();
  return (tabId: string): Promise<void> => {
    const existing = pending.get(tabId);
    if (existing) return existing;
    const request = (async () => {
      const observed = new Map<string, PreviewPreparation>();
      let expected: string | undefined;
      let finish!: () => void;
      let fail!: (error: Error) => void;
      const ready = new Promise<void>((resolve, reject) => { finish = resolve; fail = reject; });
      const check = () => {
        const state = expected === undefined ? undefined : observed.get(expected);
        if (state?.state === "ready") finish();
        else if (state?.state === "failed") fail(new Error("Preview reference indexing failed"));
      };
      const unlisten = await ports.listen(tabId, state => {
        if (observed.size === 8) observed.delete(observed.keys().next().value!);
        observed.set(state.index_id, state); check();
      });
      const refresh = async () => {
        const state = await ports.prepare(tabId);
        if (expected !== undefined && state.index_id !== expected) throw new Error("Preview session changed");
        expected = state.index_id;
        if (state.state === "ready") finish();
        else if (state.state === "failed") fail(new Error("Preview reference indexing failed"));
        else check();
      };
      const disconnect = ports.reconnect?.(() => { void refresh().catch(fail); });
      const deadline = setTimeout(() => fail(new Error("Preview reference indexing timed out")), 120_000);
      try {
        await refresh();
        await ready;
      } finally { clearTimeout(deadline); disconnect?.(); unlisten(); }
    })();
    pending.set(tabId, request);
    void request.finally(() => { if (pending.get(tabId) === request) pending.delete(tabId); }).catch(() => {});
    return request;
  };
}
