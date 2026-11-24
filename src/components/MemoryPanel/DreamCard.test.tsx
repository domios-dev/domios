import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { DreamCard } from "./DreamCard";
import { DreamDetail, DreamLive } from "./DreamDetail";
import { dreamNights, type DreamWorker } from "./dreamModel";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";

const window = new Window({ url: "http://localhost/" });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
for (const key of ["window", "document", "HTMLElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: (window as unknown as Record<string, unknown>)[key], configurable: true });
}
const copied: string[] = [];
Object.defineProperty(globalThis, "navigator", { value: { clipboard: { writeText: async (text: string) => { copied.push(text); } } }, configurable: true });
const api = { mesh_tailnet_host: null, preview_secret_paths: [], enabled: true, host: "127.0.0.1", port: 7686, token: "t", cors: false, cors_allow_origins: [] };
const entry = { id: "diary", title: "A night", body: "<insight>Insight token. Further observation.</insight><log>ORPHANS: bridged=2</log>", created_at: "2026-11-24T07:00:00Z", area: "_dreams" };
const worker: DreamWorker = { worker_id: "run", tab_id: null, intent: "dreaming:all", spawned_by: "cron", spawned_at: Date.parse(entry.created_at), status: "done", progress_text: null, touched_nodes: [], color: "", report: null, done_at: Date.parse(entry.created_at) + 120000, diary_id: entry.id };
const night = dreamNights([entry], [worker])[0];
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
let opened = 0;
await act(async () => root.render(<DreamCard night={night} selected onOpen={() => opened++} />));
const card = host.querySelector("button")!;
assert.equal(card.getAttribute("aria-pressed"), "true");
await act(async () => card.click());
assert.equal(opened, 1);

await act(async () => root.render(<DreamDetail night={night} api={api} onLog={() => {}} onRun={() => {}} />));
assert.equal(host.querySelectorAll("svg[role=img]").length, 0, "old runs never fabricate a constellation");
assert.ok(host.querySelector('[data-testid="memory-dream-counters"]'));
assert.ok(host.textContent?.includes("2 islands and orphans"));
assert.ok(!host.textContent?.includes("ORPHANS:"));
const copy = [...host.querySelectorAll("button")].find((button) => button.textContent === "Copy diary")!;
await act(async () => copy.click());
assert.deepEqual(copied, [entry.body]);

const requests: string[] = [];
baseDaemonTransport().request = async (path) => {
  requests.push(path);
  return Response.json([{ id: "a", title: "Resolved title" }]);
};
const detailed = dreamNights([entry], [{ ...worker, touched_nodes: ["a", "b"], node_actions: { a: "linked", b: "archived" }, created_edges: [{ from: "a", to: "b", relation: "uses" }] }])[0];
await act(async () => root.render(<DreamDetail night={detailed} api={api} onLog={() => {}} onRun={() => {}} />));
assert.equal(host.querySelectorAll("svg[role=img]").length, 2);
assert.equal(host.querySelectorAll("line").length, 1, "before has no created edges");
assert.ok(host.textContent?.includes("Resolved title"));
assert.ok([...host.querySelectorAll("dd")].some((cell) => cell.textContent === "b"), "unresolved memory keeps its actual id");
assert.equal(requests.length, 1, "titles resolve in a batch");
assert.ok(requests[0].includes("/api/mem/graph/labels?ids="));
assert.equal(host.querySelector('[data-testid="memory-dream-counters"]'), null);

await act(async () => root.render(<DreamLive night={{ ...night, worker: { ...worker, stage: "unknown", progress_text: "Observed progress" } }} onStop={() => {}} />));
assert.equal(host.querySelector("ol"), null, "unknown stage uses reported progress only");
assert.ok(host.textContent?.includes("Observed progress"));
let stopped = false;
await act(async () => root.render(<DreamLive night={{ ...night, worker: { ...worker, stage: "bridge_islands" } }} onStop={() => { stopped = true; }} />));
assert.equal(host.querySelector('[aria-current="step"]')?.textContent, "Bridging islands");
await act(async () => host.querySelector("button")!.click());
assert.ok(stopped);
await act(async () => root.unmount());
console.log("✓ Dreams card selection, fallback, clipboard, batched titles, real edges and live stages");
