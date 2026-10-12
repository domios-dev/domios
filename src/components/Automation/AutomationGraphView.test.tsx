import { configureOrchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AutomationGraphView } from "./AutomationGraphView";
import { attachment, fixtureGraph, FixtureTransport } from "./fixtures.testdata";
import { typedGraph, watchIdentity } from "./blueprintModel.testdata";
import type { AutomationGraph } from "./types";

configureOrchestrationTransport(orchestrationTestClient(async <T,>(_method: string, path: string) => {
  assert.equal(path, "/api/automation/node-files");
  return [] as T;
}));

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "navigator"]) {
  Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, requestAnimationFrame: () => 1, cancelAnimationFrame: () => {} });
Object.assign(globalThis, { ResizeObserver: class { observe() {} disconnect() {} } });
for (const axis of ["clientWidth", "clientHeight"] as const) {
  Object.defineProperty(window.HTMLElement.prototype, axis, { configurable: true, get(this: HTMLElement) { return this.getAttribute("aria-label") === "Automation graph" ? (axis === "clientWidth" ? 900 : 520) : 0; } });
}

function ranGraph(): AutomationGraph {
  const graph = typedGraph();
  const [watch, ...rest] = graph.nodes;
  const run = { id: "run-1", root_node: watchIdentity.id, root_transition: "t1", started_at: 1_000, parent: null };
  graph.nodes = [{ ...watch, history: [{ id: "t1", bucket: "red", accepted_at: 1_000, causal_run: run, decided_by: "adapter", adapter_id: "typesafe-jev", model: "jev-latest", confidence: 0.9, handlers: [{ handler_id: "notify-owner", status: "completed", execution_id: "n", accepted_at: 1_050 }] }] }, ...rest];
  graph.runs = [{ id: "run-1", root_node: watchIdentity.id, root_transition: "t1", started_at: 1_000, state: "completed", incomplete: false, transitions: [{ node: watchIdentity, transition_id: "t1", parent: null }] }];
  return graph;
}

const tests: Array<[string, () => Promise<void>]> = [];
let mounts = 0;
function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }
async function mount(graph: AutomationGraph = ranGraph()) {
  const transport = new FixtureTransport();
  // Cameras outlive a mount by design, so each case opens its own scenario id.
  transport.graph = { ...graph, scenarios: graph.scenarios.map((scenario) => ({ ...scenario, id: `${scenario.id}-${++mounts}` })) };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => { root.render(<AutomationGraphView adapter={transport} attachment={attachment} />); });
  const open = async () => {
    const entry = host.querySelector<HTMLButtonElement>(`button[data-scenario-id="${transport.graph.scenarios[0].id}"]`);
    assert.ok(entry, "scenario entry rendered");
    await click(entry);
  };
  await open();
  return { host, root, transport, open, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}
async function click(target: HTMLElement) { await act(async () => { target.focus(); target.click(); }); }
async function key(target: HTMLElement, code: string) { await act(async () => { target.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true })); }); }
function named<T extends HTMLElement = HTMLButtonElement>(host: HTMLElement, selector: string): T {
  const found = host.querySelector<T>(selector);
  assert.ok(found, selector);
  return found;
}
const details = (host: HTMLElement) => named<HTMLElement>(host, 'aside[role="region"]');

test("a Blueprint opens as typed Trigger, Source, Agent and Action nodes joined by exec and data wires", async () => {
  const view = await mount();
  try {
    for (const label of ["Trigger node: Every 20 s", "Source node: Run probe", "Agent node: Jev · jev-latest", "Action node: Notify owner"]) {
      assert.ok(view.host.querySelector(`button[aria-label^="${label}"]`), label);
    }
    assert.ok(view.host.querySelector('[data-wire] .bp-wire[data-kind="exec"]'));
    assert.ok(view.host.querySelector('[data-wire] .bp-wire[data-kind="data"]'));
    assert.ok(named(view.host, 'ul[aria-label="Node types"]').textContent?.includes("Agent"));
  } finally { await view.close(); }
});

test("a node opens its drawer, the drawer closes with its own x, and Escape then closes the Blueprint", async () => {
  const view = await mount();
  try {
    const agent = named(view.host, 'button[aria-label^="Agent node:"]');
    await click(agent);
    assert.equal(agent.getAttribute("aria-pressed"), "true");
    assert.equal(details(view.host).getAttribute("aria-label"), "Details: Jev · jev-latest");
    assert.equal(details(view.host).querySelector("code"), null, "the agent drawer shows no code");
    await click(named(details(view.host), 'button[aria-label="Close details"]'));
    assert.equal(view.host.querySelector("aside"), null);
    assert.equal(document.activeElement, agent, "focus returns to the opener");
    await key(agent, "Escape");
    assert.ok(view.host.querySelector('ul[aria-label="Blueprint scenarios"]'), "Escape returns to the list");
    assert.equal((document.activeElement as HTMLElement | null)?.dataset.scenarioId, view.transport.graph.scenarios[0].id);
  } finally { await view.close(); }
});

test("Escape inside the drawer closes only the drawer", async () => {
  const view = await mount();
  try {
    await click(named(view.host, 'button[aria-label^="Source node:"]'));
    await key(details(view.host), "Escape");
    assert.equal(view.host.querySelector("aside"), null);
    assert.ok(view.host.querySelector('[aria-label="Automation graph"]'), "the Blueprint stays open");
  } finally { await view.close(); }
});

test("the header close button returns to the list and a reopened Blueprint keeps its selected node", async () => {
  const view = await mount();
  try {
    await click(named(view.host, 'button[aria-label^="Action node: Notify owner"]'));
    await click(named(view.host, 'button[aria-label^="Back to"]'));
    assert.ok(view.host.querySelector('ul[aria-label="Blueprint scenarios"]'));
    await view.open();
    assert.equal(named(view.host, 'button[aria-label^="Action node: Notify owner"]').getAttribute("aria-pressed"), "true", "selection is remembered");
    assert.equal(details(view.host).getAttribute("aria-label"), "Details: Notify owner");
  } finally { await view.close(); }
});

test("clicking Replay on a run lights its path with a progress bar and Stop clears it", async () => {
  const view = await mount();
  try {
    const replay = named(view.host, 'button[aria-label^="Replay run "]');
    await click(replay);
    assert.equal(replay.getAttribute("aria-pressed"), "true");
    assert.equal(named<HTMLElement>(view.host, '[aria-label="Automation graph"]').dataset.replaying, "true");
    assert.ok(named(view.host, '[role="progressbar"][aria-label="Replay progress"]'));
    const stop = Array.from(view.host.querySelectorAll<HTMLButtonElement>('[aria-label="Replay"] button')).find((entry) => /Stop|Clear/.test(entry.textContent ?? ""));
    assert.ok(stop);
    await click(stop);
    assert.equal(view.host.querySelector('[aria-label="Replay"]'), null);
    assert.equal(named<HTMLElement>(view.host, '[aria-label="Automation graph"]').dataset.replaying, undefined);
  } finally { await view.close(); }
});

test("Runs toggles the log and its divider without replacing the graph; Stop Blueprint calls the adapter once", async () => {
  const view = await mount();
  try {
    const graph = named(view.host, '[aria-label="Automation graph"]');
    assert.ok(named(view.host, '[role="separator"][aria-label="Resize run log"]'));
    const runs = Array.from(view.host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent === "Runs")!;
    await click(runs);
    assert.equal(view.host.querySelector('[role="separator"]'), null);
    assert.equal(view.host.querySelector("#blueprint-run-log"), null);
    assert.equal(view.host.querySelector('[aria-label="Automation graph"]'), graph);
    const stop = Array.from(view.host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent === "Stop Blueprint")!;
    await click(stop);
    assert.deepEqual(view.transport.stops, [watchIdentity]);
  } finally { await view.close(); }
});

test("Team switch removes old details and subscribes with the exact attachment", async () => {
  const view = await mount();
  try {
    await click(named(view.host, 'button[aria-label^="Trigger node:"]'));
    const next = { ...attachment, team_id: "team-b", connection_id: "connection-b" };
    view.transport.graph = { ...fixtureGraph(), team_id: next.team_id, nodes: [], edges: [] };
    await act(async () => { view.root.render(<AutomationGraphView adapter={view.transport} attachment={next} />); });
    assert.equal(view.host.querySelector("aside"), null);
    assert.equal(view.transport.unsubscribeCount, 1);
    assert.deepEqual(view.transport.attachments[view.transport.attachments.length - 1], next);
  } finally { await view.close(); }
});

let finished = false;
async function main() {
  for (const [name, run] of tests) { await run(); console.log(`✓ ${name}`); }
  console.log(`${tests.length} AutomationGraphView cases passed`);
  finished = true;
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
