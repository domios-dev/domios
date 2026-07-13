// Run: npx tsx src/lib/webApi.test.ts

import { Window } from "happy-dom";

const browser = new Window({ url: "http://localhost:7686/" });
for (const key of ["window", "document", "localStorage", "sessionStorage", "DOMException"] as const) {
  Object.defineProperty(globalThis, key, {
    value: (browser as unknown as Record<string, unknown>)[key],
    configurable: true,
  });
}

let failures = 0;
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function run() {
  const {
    clearRelayTarget,
    invalidateWebRequestContext,
    setRelayTarget,
    webFetch,
  } = await import("./webApi");
  let requestSignal: AbortSignal | null = null;
  globalThis.fetch = (_input, init) => {
    requestSignal = init?.signal ?? null;
    return new Promise<Response>((_resolve, reject) => {
      requestSignal?.addEventListener("abort", () => {
        reject(new DOMException("aborted", "AbortError"));
      });
    });
  };

  const stalePaneRequest = webFetch("/api/panes/blackstar-pane/chat-raw/start", { method: "POST" });
  invalidateWebRequestContext();
  assert((requestSignal as AbortSignal | null)?.aborted, "peer switch aborts the old pane request");
  try {
    await stalePaneRequest;
    assert(false, "aborted old-pane request must reject");
  } catch (error) {
    assert(error instanceof DOMException && error.name === "AbortError", "old request rejects as AbortError");
  }

  const { loadCachedStateSync, saveStateSnapshot } = await import("./stateCache");
  const state = (id: string) => ({
    workspaces: [{ id, name: id, tabs: [], active_tab_id: null, cwd: null }],
    panes: {},
    active_workspace_id: id,
    focused_pane_id: null,
  });
  clearRelayTarget();
  saveStateSnapshot(state("blackstar"));
  setRelayTarget("mac-air-id", "Mac-Air");
  assert(loadCachedStateSync() === null, "self-host pane cache is invisible under Mac-Air");
  saveStateSnapshot(state("mac-air"));
  clearRelayTarget();
  assert(loadCachedStateSync()?.active_workspace_id === "blackstar", "self cache survives peer round-trip");
}

run()
  .then(() => console.log("✓ webApi peer request cancellation tests green"))
  .catch((error) => {
    failures += 1;
    console.error(`✗ webApi peer request cancellation: ${error instanceof Error ? error.message : String(error)}`);
  })
  .finally(() => process.exit(failures === 0 ? 0 : 1));
