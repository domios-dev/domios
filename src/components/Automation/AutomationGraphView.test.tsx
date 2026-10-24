import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AutomationGraphView } from "./AutomationGraphView";
import { fixtureGraph, FixtureTransport } from "./fixtures.testdata";
import capturedWatch from "./blueprintWatchCaptured.testdata.json";
import type { AutomationAttachment, AutomationGraph } from "./types";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "navigator"]) {
  Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, requestAnimationFrame: () => 1, cancelAnimationFrame: () => {} });
Object.assign(globalThis, { ResizeObserver: class { observe() {} disconnect() {} } });
for (const axis of ["clientWidth", "clientHeight"] as const) {
  Object.defineProperty(window.HTMLElement.prototype, axis, { configurable: true, get(this: HTMLElement) { return this.classList.contains("bp-canvas") ? (axis === "clientWidth" ? 900 : 520) : 0; } });
}
const nativeMessageChannel = globalThis.MessageChannel;
class TestMessageChannel {
  port1 = { onmessage: null as (() => void) | null, close() {} };
  port2 = { postMessage: () => queueMicrotask(() => this.port1.onmessage?.()), close() {} };
}
Object.defineProperty(globalThis, "MessageChannel", { configurable: true, value: TestMessageChannel });

const watch = capturedWatch as AutomationGraph;
const attachment: AutomationAttachment = { connection_id: "connection-watch", workspace_id: watch.workspace_id, team_id: watch.team_id };
const root = watch.scenarios[0].root;

const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }
async function settle() { for (let index = 0; index < 4; index++) await act(async () => { await new Promise<void>((resolve) => queueMicrotask(resolve)); }); }
async function mount(graph: AutomationGraph = watch) {
  const transport = new FixtureTransport();
  transport.graph = structuredClone(graph);
  const host = document.createElement("div");
  document.body.append(host);
  const reactRoot = createRoot(host);
  await act(async () => { reactRoot.render(<AutomationGraphView adapter={transport} attachment={attachment} />); });
  const open = async () => {
    const entry = host.querySelector<HTMLButtonElement>(`button[data-scenario-id="${transport.graph.scenarios[0].id}"]`);
    assert.ok(entry, "scenario entry rendered");
    await click(entry);
    await settle();
  };
  await open();
  return { host, root: reactRoot, transport, open, close: async () => { await act(async () => reactRoot.unmount()); host.remove(); } };
}
async function click(target: HTMLElement) { await act(async () => { target.focus(); target.click(); }); }
async function key(target: HTMLElement, code: string) { await act(async () => { target.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true })); }); }
function named<T extends HTMLElement = HTMLButtonElement>(host: HTMLElement, selector: string): T {
  const found = host.querySelector<T>(selector);
  assert.ok(found, selector);
  return found;
}
const button = (host: HTMLElement, text: string) => {
  const found = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => (entry.getAttribute("aria-label") ?? entry.textContent) === text);
  assert.ok(found, text);
  return found;
};
const details = (host: HTMLElement) => host.querySelectorAll<HTMLElement>('aside[role="region"]');
const graphRegion = (host: HTMLElement) => named<HTMLElement>(host, ".bp-canvas");

test("a watch opens as typed nodes joined by labelled exec pins, with its retained run on every node", async () => {
  const view = await mount();
  try {
    for (const label of ["Node source", "Node decide", "Node notify", "Node Stop", "Node Result"]) assert.ok(button(view.host, label), label);
    assert.ok(view.host.querySelector('.bp-wire[data-kind="exec"]'));
    assert.ok(button(view.host, "Node decide").textContent?.includes("Classify"), "each node names its own kind, so no legend is needed");
    assert.ok(button(view.host, "Node decide").textContent?.includes("passed"), "the decision shows its recorded outcome");
    assert.ok(!view.host.querySelector('.bp-node[data-state="waiting"]'), "a retained run never shows waiting nodes");
    assert.equal(graphRegion(view.host).dataset.replaying, "true", "the recorded path is lit");
  } finally { await view.close(); }
});

test("a node opens its drawer, another node replaces it at once, x closes it and Escape then closes the Blueprint", async () => {
  const view = await mount();
  try {
    const source = button(view.host, "Node source");
    await click(source);
    assert.equal(source.getAttribute("aria-pressed"), "true");
    assert.deepEqual([...details(view.host)].map((entry) => entry.getAttribute("aria-label")), ["Details: source"]);
    await click(button(view.host, "Node decide"));
    assert.deepEqual([...details(view.host)].map((entry) => entry.getAttribute("aria-label")), ["Details: decide"], "selection switches in the same click");
    assert.equal(source.getAttribute("aria-pressed"), "false");
    const decide = button(view.host, "Node decide");
    await click(named(details(view.host)[0], 'button[aria-label="Close details"]'));
    assert.equal(details(view.host).length, 0);
    assert.equal(document.activeElement, decide, "focus returns to the opener");
    await key(decide, "Escape");
    assert.ok(view.host.querySelector('ul[aria-label="Blueprint scenarios"]'), "Escape returns to the list");
    assert.equal((document.activeElement as HTMLElement | null)?.dataset.scenarioId, view.transport.graph.scenarios[0].id);
  } finally { await view.close(); }
});

test("Escape inside the drawer closes only the drawer", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Node source"));
    await key(details(view.host)[0], "Escape");
    assert.equal(details(view.host).length, 0);
    assert.ok(view.host.querySelector(".bp-canvas"), "the Blueprint stays open");
  } finally { await view.close(); }
});

test("the header Back returns to the list and a reopened Blueprint keeps its selected node", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Node notify"));
    await click(named(view.host, 'button[aria-label^="Back to"]'));
    assert.ok(view.host.querySelector('ul[aria-label="Blueprint scenarios"]'));
    await view.open();
    assert.equal(button(view.host, "Node notify").getAttribute("aria-pressed"), "true", "selection is remembered");
    assert.equal(details(view.host)[0]?.getAttribute("aria-label"), "Details: notify");
  } finally { await view.close(); }
});

test("Runs toggles the run list and journal without replacing the graph; Stop Blueprint calls the adapter once", async () => {
  const view = await mount();
  try {
    const graph = graphRegion(view.host);
    assert.ok(named(view.host, '[role="separator"][aria-label="Resize run log"]'));
    assert.ok(named(view.host, '[aria-label="Selected run journal"]').textContent?.includes("passed"));
    await click(Array.from(view.host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent?.startsWith("Runs"))!);
    assert.equal(view.host.querySelector('[role="separator"]'), null);
    assert.equal(view.host.querySelector("#blueprint-run-log"), null);
    assert.equal(view.host.querySelector(".bp-canvas"), graph);
    await click(button(view.host, "Stop Blueprint"));
    assert.deepEqual(view.transport.stops, [root]);
  } finally { await view.close(); }
});

test("Team switch removes old details and subscribes with the exact attachment", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Node source"));
    const next = { ...attachment, team_id: "team-b", connection_id: "connection-b" };
    view.transport.graph = { ...fixtureGraph(), team_id: next.team_id, nodes: [], edges: [] };
    await act(async () => { view.root.render(<AutomationGraphView adapter={view.transport} attachment={next} />); });
    assert.equal(details(view.host).length, 0);
    assert.equal(view.transport.unsubscribeCount, 1);
    assert.deepEqual(view.transport.attachments[view.transport.attachments.length - 1], next);
  } finally { await view.close(); }
});

test("a background graph failure is a quiet accessible unavailable state", async () => {
  const transport = new FixtureTransport();
  transport.readOverride = async () => { throw new Error("opaque graph refresh failure"); };
  const host = document.createElement("div");
  document.body.append(host);
  const reactRoot = createRoot(host);
  try {
    await act(async () => { reactRoot.render(<AutomationGraphView adapter={transport} attachment={attachment} />); });
    await act(async () => { await transport.readStarted.promise; await Promise.resolve(); });
    await act(async () => {});
    assert.ok(host.querySelector('[role="status"]'), "the unavailable graph is announced as a status");
    assert.equal(host.querySelector('[role="alert"]'), null, "background graph failure does not create an alert");
  } finally {
    await act(async () => reactRoot.unmount());
    host.remove();
  }
});

test("a failed background refresh keeps a loaded Blueprint without an error notice", async () => {
  const view = await mount();
  let failRead!: () => void;
  const failedReadStarted = new Promise<void>((resolve) => { failRead = resolve; });
  try {
    assert.ok(view.host.querySelector(".bp-canvas"), "the Blueprint is loaded");
    view.transport.readOverride = async () => {
      failRead();
      throw new Error("opaque scoped graph refresh failure");
    };
    await act(async () => {
      view.transport.changed?.();
      await failedReadStarted;
      await Promise.resolve();
    });
    assert.ok(view.host.querySelector(".bp-canvas"), "the last good graph stays available");
    assert.equal(view.host.querySelector('[role="alert"]'), null, "a cached graph refresh failure does not create an alert");
  } finally { await view.close(); }
});

let finished = false;
async function main() {
  try {
    for (const [name, run] of tests) { await run(); console.log(`✓ ${name}`); }
    console.log(`${tests.length} AutomationGraphView cases passed`);
  } finally {
    finished = true;
    Object.defineProperty(globalThis, "MessageChannel", { configurable: true, value: nativeMessageChannel });
  }
}
process.once("beforeExit", () => {
  if (finished) return;
  console.error("AutomationGraphView exited before its expected state event");
  process.exitCode = 1;
});
void main().catch((error: unknown) => {
  finished = true;
  console.error(error);
  process.exitCode = 1;
});
