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
    setTokenFor,
    webFetch,
    webUpload,
  } = await import("./webApi");
  const upload = new Blob(["image-bytes"], { type: "image/png" });
  let uploadUrl = "";
  let uploadInit: RequestInit | undefined;
  globalThis.fetch = async (input, init) => {
    uploadUrl = String(input);
    uploadInit = init;
    return new Response(JSON.stringify({ path: "C:/uploads/photo.png", size: upload.size }), { status: 200 });
  };
  setTokenFor("http://localhost:7686", "test-token");
  const stored = await webUpload(upload, "phone photo.png");
  assert(uploadUrl.endsWith("/api/upload"), "upload uses the multipart endpoint");
  assert(uploadInit?.body instanceof FormData, "upload uses browser-native multipart encoding");
  const uploadPart = uploadInit.body.get("file");
  assert(uploadPart instanceof Blob, "multipart upload contains the file field");
  assert((uploadPart as Blob & { name?: string }).name === "phone photo.png", "multipart upload preserves filename");
  assert(
    !new Headers(uploadInit.headers).has("Content-Type"),
    "upload lets the browser generate the multipart boundary",
  );
  assert(stored.path === "C:/uploads/photo.png" && stored.size === upload.size, "upload returns the daemon path");

  let requestSignal: AbortSignal | null = null;
  globalThis.fetch = (_input, init) => {
    requestSignal = init?.signal ?? null;
    return new Promise<Response>((_resolve, reject) => {
      requestSignal?.addEventListener("abort", () => {
        reject(new DOMException("aborted", "AbortError"));
      });
    });
  };

  const staleTabRequest = webFetch("/api/tabs/blackstar-tab/chat-raw/start", { method: "POST" });
  invalidateWebRequestContext();
  assert((requestSignal as AbortSignal | null)?.aborted, "peer switch aborts the old Tab request");
  try {
    await staleTabRequest;
    assert(false, "aborted old-tab request must reject");
  } catch (error) {
    assert(error instanceof DOMException && error.name === "AbortError", "old request rejects as AbortError");
  }

  const { loadCachedStateSync, saveStateSnapshot } = await import("./stateCache");
  const state = (id: string) => ({
    workspaces: [{ id, name: id, tabs: [], active_tab_id: null, cwd: null }],
    tabs: {},
    active_workspace_id: id,
    focused_tab_id: null,
  });
  clearRelayTarget();
  saveStateSnapshot(state("blackstar"));
  setRelayTarget("mac-air-id", "Mac-Air");
  assert(loadCachedStateSync() === null, "self-host Tab cache is invisible under Mac-Air");
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
