import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AutomationCanvas } from "./AutomationCanvas";
import { createAutomationCamera } from "./useAutomationViewport";
import { AutomationDetails } from "./AutomationDetails";
import { RunLog } from "./RunLog";
import { fixtureGraph } from "./fixtures.testdata";
import { useRunReplay } from "./useRunReplay";

async function main() {
const window = new Window({ url: "http://localhost" });
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "navigator", "KeyboardEvent"]) Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let reduced = false;
const matchMedia = window.matchMedia.bind(window);
window.matchMedia = (media) => {
  const query = matchMedia(media);
  Object.defineProperty(query, "matches", { get: () => reduced });
  return query;
};
let frame: FrameRequestCallback | null = null;
let scheduled = 0;
Object.assign(globalThis, { requestAnimationFrame: (callback: FrameRequestCallback) => { frame = callback; return ++scheduled; }, cancelAnimationFrame: () => { frame = null; } });
const graph = fixtureGraph();
const node = { ...graph.nodes[0], detail: { definition: { probe: "printf ready", language: "bash" } } };
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
const click = async (button: HTMLButtonElement) => act(async () => { button.focus(); button.click(); });
const button = (name: string) => {
  const found = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent === name);
  assert.ok(found, name);
  return found;
};
try {
  let closed = 0;
  await act(async () => root.render(<AutomationDetails node={node} nodes={[node]} onClose={() => closed++} />));
  const facts = Array.from(host.querySelectorAll("dt")).map((term) => term.textContent);
  for (const excluded of ["Status", "Schedule", "Cadence", "Expiry"]) assert.ok(!facts.includes(excluded));
  assert.equal(host.querySelector("textarea, input, [contenteditable=true]"), null);
  const code = host.querySelector("code");
  assert.ok(code?.textContent?.includes("printf ready"));
  assert.ok(code?.querySelector("span"), "code goes through the shared syntax highlighter");
  const opener = button("Fullscreen");
  await click(opener);
  assert.ok(document.querySelector('[role="dialog"][aria-label="Fullscreen code"] code'));
  await act(async () => { window.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(closed, 0, "fullscreen Escape leaves the node inspector open");
  assert.equal(document.activeElement, opener);
  const classifier = { adapter_id: "typesafe-jev", model: "jev-latest" };
  const classified = { ...node, detail: { definition: { ...node.detail.definition, question: "branch-a: first path; branch-b: second path", buckets: [{ name: "branch-a", pattern: null }, { name: "branch-b", pattern: null }] } } };
  await act(async () => root.render(<AutomationDetails node={classified} nodes={[classified]} classifier={classifier} onClose={() => closed++} />));
  assert.ok(host.querySelector('header [aria-label="AI: Jev \u00b7 jev-latest"]'));
  assert.equal(host.querySelector('[aria-label="Routes"] dl')?.children.length, 2);
  const classifiedGraph = { ...graph, configured_fallback: classifier, nodes: [classified, graph.nodes[2]] };
  await act(async () => root.render(<AutomationCanvas graph={classifiedGraph} selected={null} camera={createAutomationCamera()} onSelect={() => {}} />));
  assert.ok(host.querySelector('button[aria-label^="Agent node:"] [aria-label="AI: Jev \u00b7 jev-latest"]'));
  const withHistory = { ...graph, nodes: [{ ...node, history: [
    { id: "first", bucket: "branch-a", accepted_at: 100, handlers: [] },
    { id: "second", decided_by: "adapter" as const, adapter_id: "typesafe-jev", model: "jev-latest", confidence: 0.87, bucket: "branch-b", accepted_at: 200, handlers: [{ handler_id: "handler-c", status: "completed", execution_id: "execution", accepted_at: 201 }] },
  ] }] };
  let replayed: string[] = [];
  await act(async () => root.render(<RunLog graph={withHistory} selected={null} onReplay={(run) => { replayed = run.events.flatMap((event) => event.edgeId ? [event.edgeId] : []); }} />));
  const runs = host.querySelectorAll('section > ol > li');
  assert.equal(runs.length, 2);
  assert.ok(runs[0].querySelector('[aria-label="AI: Jev \u00b7 jev-latest"]'));
  assert.ok(host.querySelector('input[aria-label="Run log height"]'));
  await click(runs[0].querySelector<HTMLButtonElement>("button")!);
  assert.deepEqual(replayed, ["edge-b"]);
  const selection = { key: "run", edges: ["edge-a", "edge-b"] };
  function Replay() {
    const state = useRunReplay(selection);
    return <output>{state.active ?? "static"}</output>;
  }
  await act(async () => root.render(<Replay />));
  assert.ok(frame);
  await act(async () => { frame?.(100); });
  assert.equal(host.querySelector("output")?.textContent, "edge-a");
  await act(async () => { frame?.(950); });
  assert.equal(host.querySelector("output")?.textContent, "edge-b");
  await act(async () => root.render(<div />));
  reduced = true;
  const before = scheduled;
  await act(async () => root.render(<Replay />));
  assert.equal(host.querySelector("output")?.textContent, "static");
  assert.equal(scheduled, before, "reduced motion schedules no travel frames");
} finally {
  await act(async () => root.unmount());
  host.remove();
}
console.log("Node inspector, code fullscreen, run log and replay wiring passed");

}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
