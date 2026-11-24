import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../../lib/bridge";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";
import { MemoryDreamsTab } from "./MemoryDreamsTab";

const window = new Window({ url: "http://localhost/" });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
for (const key of ["window", "document", "HTMLElement", "HTMLDialogElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: (window as unknown as Record<string, unknown>)[key], configurable: true });
}
class ResizeObserverStub {
  constructor(private callback: (entries: { contentRect: { width: number } }[]) => void) {}
  observe() { this.callback([{ contentRect: { width: 1000 } }]); }
  disconnect() {}
}
Object.defineProperty(globalThis, "ResizeObserver", { value: ResizeObserverStub, configurable: true });
__setListenForTest(async () => () => {});
globalThis.fetch = (() => { throw new Error("Raw daemon fetch bypass"); }) as typeof fetch;
const seen: string[] = [];
const at = Date.now();
const entry = { id: "diary", title: "A night", body: "<insight>Remembered token.</insight><log>ORPHANS: bridged=2</log>", created_at: new Date(at).toISOString(), area: "_dreams" };
baseDaemonTransport().request = async (path) => {
  seen.push(path);
  if (path.includes("/api/mem/dream/diary")) {
    const offset = new URL(path, "http://test").searchParams.get("offset");
    return Response.json(offset === "0" ? { items: [entry], total: 2, next_offset: 1 } : { items: [{ ...entry, id: "older", created_at: new Date(at - 86400000).toISOString(), body: "<insight>WIP: dreaming</insight>" }], total: 2, next_offset: null });
  }
  if (path === "/api/crons") return Response.json([]);
  if (path === "/api/mem/workers") return Response.json([
    { worker_id: "run", tab_id: null, intent: "dreaming:all", spawned_by: "cron", spawned_at: at + 1, status: "error", progress_text: null, touched_nodes: [], color: "", report: "Failure token", done_at: at + 60000 },
  ]);
  return Response.json({});
};
const api = { mesh_tailnet_host: null, preview_secret_paths: [], enabled: true, host: "127.0.0.1", port: 7686, token: "t", cors: false, cors_allow_origins: [] };
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
await act(async () => root.render(<MemoryDreamsTab api={api} />));
assert.equal(host.querySelectorAll('[data-testid="memory-dream-card"]').length, 3, "diaries and stopped runs share one nights list");
assert.equal(host.querySelector('[data-testid="memory-dream-detail"]')?.getAttribute("data-dream-id"), "diary", "complete diary is the default selection");
const stopped = host.querySelector<HTMLElement>('[data-dream-id="run"]')!;
assert.ok(stopped.textContent?.includes("Stopped"));
await act(async () => stopped.click());
assert.equal(host.querySelector('[data-testid="memory-dream-detail"]')?.getAttribute("data-dream-id"), "run");
assert.ok(host.querySelector('[data-testid="memory-dream-detail"]')?.textContent?.includes("Failure token"));
assert.ok(host.textContent?.includes("Incomplete"));
assert.equal(seen.filter((path) => path.includes("/api/mem/dream/diary")).length, 2, "the overview loads all pages in its time window");
assert.ok(seen.some((path) => path.includes("limit=100")));
assert.ok(!seen.some((path) => path.includes("/api/mem/search")), "diaries never use relevance search");
await act(async () => root.unmount());
__setListenForTest(null);
console.log("✓ Dreams transport authority, diary pagination, stopped runs and selected panel");
