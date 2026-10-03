import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AutomationGraphView } from "./AutomationGraphView";
import { attachment, chainGraph, fixtureGraph, fixtureNode, FixtureTransport, identity, multiScenarioGraph, sentinelIdentity } from "./fixtures.testdata";
import { automationKey, type AutomationGraph, type CommandResultMetadata } from "./types";
import { COMMAND_FACT, commandResultLabel } from "./CommandResultDetails";
import { frameAutomation, layoutAutomationOrientations, NODE_WIDTH, preferredOrientation, READABLE_SCALE } from "./layout";
import { scopedAutomationGraph } from "./model";
import { formatLayerTransform } from "../Canvas/useCanvasViewport";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "navigator"]) {
  Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const viewportSize = { width: 800, height: 480 };
const resized: Array<() => void> = [];
Object.assign(globalThis, { ResizeObserver: class { constructor(private readonly changed: () => void) {} observe() { resized.push(this.changed); } disconnect() { resized.splice(resized.indexOf(this.changed), 1); } } });
for (const axis of ["clientWidth", "clientHeight"] as const) {
  Object.defineProperty(window.HTMLElement.prototype, axis, { configurable: true, get(this: HTMLElement) { return this.getAttribute("aria-label") === "Automation graph" ? viewportSize[axis === "clientWidth" ? "width" : "height"] : 0; } });
}
async function resize(width: number, height: number) {
  Object.assign(viewportSize, { width, height });
  await act(async () => { for (const changed of [...resized]) changed(); });
}
function cameraLayer(host: HTMLElement) {
  const layer = host.querySelector<HTMLElement>('[aria-label="Automation graph"] > div');
  assert.ok(layer, "graph camera layer rendered");
  return layer;
}
function fitted(graph: AutomationGraph, width: number, height: number, whole = false) {
  const layouts = layoutAutomationOrientations(scopedAutomationGraph(graph, attachment));
  const size = { width, height };
  const fit = frameAutomation(layouts[preferredOrientation(layouts, size)], size, whole);
  assert.ok(fit);
  return formatLayerTransform(fit);
}
function camera(host: HTMLElement) {
  const match = /translate\((-?[\d.e-]+)px, (-?[\d.e-]+)px\) scale\((-?[\d.e-]+)\)/.exec(cameraLayer(host).style.transform);
  assert.ok(match, "camera transform painted");
  return { x: Number(match[1]), y: Number(match[2]), scale: Number(match[3]) };
}
/** On-screen rectangles of the rendered participant cards, projected through the painted camera. */
function projectedCards(host: HTMLElement, names: readonly string[]) {
  const t = camera(host);
  return names.map((name) => {
    const card = host.querySelector<HTMLElement>(`[aria-label="Automation graph"] button[aria-label*=": ${name} · "]`);
    assert.ok(card, `${name} card rendered`);
    const [left, top, width, height] = [card.style.left, card.style.top, card.style.width, card.style.height].map((value) => Number.parseFloat(value));
    return { x: left * t.scale + t.x, y: top * t.scale + t.y, width: width * t.scale, height: height * t.scale };
  });
}
function readable(cards: ReturnType<typeof projectedCards>, width: number, height: number) {
  for (const card of cards) {
    assert.ok(card.x >= 0 && card.y >= 0 && card.x + card.width <= width && card.y + card.height <= height, "card fully inside the measured canvas");
    assert.ok(card.width >= NODE_WIDTH * READABLE_SCALE, `projected card width ${card.width} is readable`);
  }
}
const CHAIN = ["Chain root", "Chain stage", "Chain worker"] as const;
function flows(cards: ReturnType<typeof projectedCards>, axis: "down" | "right") {
  cards.slice(1).forEach((card, index) => {
    const previous = cards[index];
    assert.ok(axis === "down" ? card.y >= previous.y + previous.height : card.x >= previous.x + previous.width, `chain flows ${axis}`);
  });
}
function technical(panel: HTMLElement) {
  const toggle = panel.querySelector<HTMLButtonElement>("button[aria-expanded]");
  assert.ok(toggle, "technical disclosure rendered");
  const region = document.getElementById(toggle.getAttribute("aria-controls") ?? "");
  assert.ok(region && panel.contains(region), "disclosure controls its own region");
  return { toggle, region };
}
const tests: Array<[string, () => Promise<void>]> = [];
function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }
async function mount(transport = new FixtureTransport()) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => { root.render(<AutomationGraphView adapter={transport} attachment={attachment} />); });
  const scenario = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent?.includes(transport.graph.scenarios[0].name));
  assert.ok(scenario);
  await act(async () => scenario.click());
  return { host, root, transport, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}
function button(host: HTMLElement, text: string) {
  const found = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent?.includes(text));
  assert.ok(found, `button ${text} rendered`);
  return found;
}
async function click(target: HTMLButtonElement) { await act(async () => { target.focus(); target.click(); }); }
function details(host: HTMLElement) {
  const panel = host.querySelector<HTMLElement>('aside[role="region"]');
  assert.ok(panel, "details rendered");
  return panel;
}

test("branched graph renders declared nodes and exact configured versus execution edges", async () => {
  const view = await mount();
  try {
    assert.equal(view.host.querySelectorAll('[data-edge-kind="configured"]').length, 3);
    assert.equal(view.host.querySelectorAll('[data-edge-kind="observed"]').length, 1);
    assert.ok(view.host.textContent?.includes(view.transport.graph.nodes[1].schedule!));
    await click(button(view.host, "Watcher A"));
    const panel = details(view.host);
    assert.ok(panel.textContent?.includes(view.transport.graph.nodes[0].purpose!));
    for (const outcome of view.transport.graph.nodes[0].outcomes) assert.ok(panel.textContent?.includes(outcome.bucket));
    assert.ok(panel.textContent?.includes("accepted-a"));
    assert.ok(panel.textContent?.includes("15000 ms"));
    assert.ok(panel.textContent?.includes("route-a"));
    assert.ok(!panel.textContent?.includes("launch-a"), "launch is attached only to its exact source");
    assert.ok(panel.textContent?.includes("handler-a · cron · available"));
    assert.ok(panel.textContent?.includes("handler-b · sentinel · unavailable"));
    assert.ok(panel.textContent?.includes("failed"));
    await click(button(panel, "Schedule A"));
    assert.ok(details(view.host).textContent?.includes("execution-a"));
  } finally { await view.close(); }
});

test("hidden Worker and missing target remain selectable without changing tab visibility", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    await click(button(details(view.host), "Worker A"));
    assert.ok(details(view.host).textContent?.includes("Hidden"));
    assert.ok(details(view.host).textContent?.includes("idle"));
    const { toggle, region } = technical(details(view.host));
    assert.equal(toggle.getAttribute("aria-expanded"), "false");
    assert.equal(region.hidden, true, "technical identity starts collapsed");
    await click(toggle);
    assert.equal(toggle.getAttribute("aria-expanded"), "true");
    assert.equal(region.hidden, false);
    assert.equal(document.activeElement, toggle, "disclosure keeps keyboard focus");
    assert.ok(region.textContent?.includes(view.transport.graph.nodes[2].executor!.provider_id!));
    assert.ok(details(view.host).textContent?.includes(view.transport.graph.nodes[2].executor!.model!));
    assert.ok(details(view.host).textContent?.includes("chat_backend"));
    await click(button(view.host, "missing-a"));
    assert.ok(details(view.host).textContent?.includes("missing"));
    assert.ok(details(view.host).textContent?.includes("Unknown"));
    assert.ok(details(view.host).textContent?.includes("owner-a"));
    assert.equal(view.transport.stops.length, 0);
  } finally { await view.close(); }
});

test("authoritative event updates selected Sentinel and live hidden Worker", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    assert.ok(details(view.host).textContent?.includes("paused"));
    view.transport.graph = { ...fixtureGraph(), revision: "revision-b", nodes: fixtureGraph().nodes.map((node) => ({ ...node, state: node.identity.kind === "worker" ? "working" : "active" })) };
    await act(async () => { view.transport.changed!(); });
    assert.ok(details(view.host).textContent?.includes("active"));
    await click(button(view.host, "Worker A"));
    assert.ok(details(view.host).textContent?.includes("working"));
    assert.ok(details(view.host).textContent?.includes("Hidden"));
    assert.equal(view.transport.reads, 2);
  } finally { await view.close(); }
});

test("stop stays pending until accepted record is read and carries exact identity", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    await click(button(view.host, "Stop Sentinel"));
    assert.equal(button(view.host, "Stopping").disabled, true);
    assert.ok(details(view.host).textContent?.includes("paused"));
    assert.deepEqual(view.transport.stops, [sentinelIdentity]);
    view.transport.graph = { ...fixtureGraph(), nodes: fixtureGraph().nodes.map((node) => automationKey(node.identity) === automationKey(sentinelIdentity) ? { ...node, state: "stopped", stop: { allowed: false, reason: "reason-a" } } : node) };
    await act(async () => { view.transport.stopResult.resolve(); });
    assert.ok(details(view.host).textContent?.includes("stopped"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, true);
    assert.ok(details(view.host).textContent?.includes("reason-a"));
  } finally { await view.close(); }
});

test("stop rejection renders truthful error without raw error payload", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    await click(button(view.host, "Stop Sentinel"));
    await act(async () => { view.transport.stopResult.reject(new Error("private-payload-a")); });
    assert.ok(view.host.querySelector('[role="alert"]'));
    assert.ok(details(view.host).textContent?.includes("paused"));
    assert.ok(!view.host.textContent?.includes("private-payload-a"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, false);
  } finally { await view.close(); }
});

test("list native buttons provide keyboard selection and detail Escape restores focus", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Show list"));
    assert.ok(view.host.querySelector('ul[aria-label="Automation participants"]'));
    const selected = button(view.host, "Worker A");
    assert.equal(selected.type, "button");
    assert.ok(selected.tabIndex >= 0, "native button remains in keyboard focus order");
    await click(selected);
    assert.equal(document.activeElement, details(view.host));
    await act(async () => { details(view.host).dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    assert.equal(view.host.querySelector("aside"), null);
    assert.equal(document.activeElement, selected);
  } finally { await view.close(); }
});

test("failed refresh marks retained records stale and disables stop", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    view.transport.readOverride = async () => { throw new Error("secret-a"); };
    await act(async () => { view.transport.changed!(); });
    assert.ok(view.host.querySelector('[role="alert"]'));
    assert.ok(details(view.host).textContent?.includes("Stale"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, true);
    assert.ok(!view.host.textContent?.includes("secret-a"));
  } finally { await view.close(); }
});

test("Team switch removes old details and subscribes with exact attachment", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Watcher A"));
    const next = { ...attachment, team_id: "team-b", connection_id: "connection-b" };
    view.transport.graph = { ...fixtureGraph(), team_id: next.team_id, nodes: [], edges: [] };
    await act(async () => { view.root.render(<AutomationGraphView adapter={view.transport} attachment={next} />); });
    assert.equal(view.host.querySelector("aside"), null);
    assert.ok(!view.host.textContent?.includes("Watcher A"));
    assert.equal(view.transport.unsubscribeCount, 1);
    assert.deepEqual(view.transport.attachments[view.transport.attachments.length - 1], next);
  } finally { await view.close(); }
});

test("authoritative lifecycle values and unknown Worker stay explicit with Team filtering", async () => {
  const transport = new FixtureTransport();
  const foreign = { ...fixtureGraph().nodes[2], name: "Foreign participant", identity: { ...fixtureGraph().nodes[2].identity, team_id: "foreign-team" } };
  transport.graph = { ...fixtureGraph(), nodes: [...fixtureGraph().nodes.map((node) => node.identity.kind === "worker" ? { ...node, state: null } : node), foreign] };
  const view = await mount(transport);
  try {
    assert.ok(!view.host.textContent?.includes(foreign.name));
    await click(button(view.host, "Worker A"));
    assert.ok(details(view.host).textContent?.includes("Unknown"));
    await click(button(view.host, "Watcher A"));
    for (const state of ["active", "stopped", "expired"]) {
      transport.graph = { ...transport.graph, nodes: transport.graph.nodes.map((node) => node.identity.kind === "sentinel" ? { ...node, state } : node) };
      await act(async () => { transport.changed!(); });
      assert.ok(details(view.host).textContent?.includes(state));
    }
    transport.graph = { ...transport.graph, nodes: transport.graph.nodes.map((node) => node.identity.kind === "sentinel" ? { ...node, availability: "unavailable" as const, stop: { allowed: false, reason: null } } : node) };
    await act(async () => { transport.changed!(); });
    assert.ok(details(view.host).textContent?.includes("unavailable"));
    assert.equal(button(view.host, "Stop Sentinel").disabled, true);
  } finally { await view.close(); }
});

test("unconnected Team automation and host monitor stay absent from graph and list", async () => {
  const transport = new FixtureTransport();
  const unconnected = [fixtureNode(identity("cron", "orphan-cron"), "Orphan schedule"), fixtureNode(identity("worker", "orphan-worker"), "Orphan worker"), fixtureNode(identity("sentinel", "host-monitor"), "Host monitor")];
  transport.graph = { ...fixtureGraph(), nodes: [...fixtureGraph().nodes, ...unconnected] };
  const view = await mount(transport);
  try {
    for (const node of unconnected) assert.ok(!view.host.textContent?.includes(node.name));
    await click(button(view.host, "Show list"));
    for (const node of unconnected) assert.ok(!view.host.textContent?.includes(node.name));
    assert.equal(view.host.querySelectorAll('ul[aria-label="Automation participants"] > li').length, 4);
    await click(button(view.host, "Watcher A"));
    assert.ok(details(view.host).textContent?.includes("stored_agent_definition"));
    assert.ok(details(view.host).textContent?.includes("owner-a"));
  } finally { await view.close(); }
});

test("one named Sentinel entry opens its internal stages while shared execution keeps exact evidence", async () => {
  const transport = new FixtureTransport();
  transport.graph = multiScenarioGraph();
  const view = await mount(transport);
  try {
    assert.ok(view.host.textContent?.includes("Internal stage A"));
    assert.equal(view.host.querySelector("h2")?.textContent, `${transport.graph.scenarios[0].name} · Sentinel graph`);
    assert.ok(!view.host.textContent?.includes("Watcher B"));
    assert.equal(view.host.querySelectorAll('[data-edge-kind="observed"]').length, 1);
    await click(button(view.host, "All Sentinels"));
    assert.equal(view.host.querySelectorAll('ul[aria-label="Sentinel scenarios"] > li').length, 2);
    assert.ok(!view.host.textContent?.includes("Internal stage A"), "internal stage is not a separate scenario entry");
    await click(button(view.host, "Sentinel scenario B"));
    assert.equal(view.host.querySelector("h2")?.textContent, `${transport.graph.scenarios[1].name} · Sentinel`);
    assert.ok(view.host.textContent?.includes("Watcher B"));
    assert.ok(!view.host.textContent?.includes("Watcher A"));
    assert.ok(!view.host.textContent?.includes("Internal stage A"));
    assert.equal(view.host.querySelectorAll('[data-edge-kind="observed"]').length, 1);
    await click(button(view.host, "Schedule A"));
    assert.ok(details(view.host).textContent?.includes("execution-a"));
    assert.ok(!details(view.host).textContent?.includes("route-a"));
    transport.graph = { ...transport.graph, scenarios: [transport.graph.scenarios[0]] };
    await act(async () => { transport.changed!(); });
    assert.ok(view.host.querySelector('[role="alert"]')?.textContent?.includes("selected Sentinel is unavailable"));
    assert.equal(view.host.querySelector("aside"), null);
    assert.equal(view.host.querySelectorAll('[data-edge-kind]').length, 0);
  } finally { await view.close(); }
});

test("backend-refused command handler and incomplete snapshot remain explicit", async () => {
  const transport = new FixtureTransport();
  const refused = { id: "bounded-command-fixture", type: "command", target: null, availability: "unavailable" as const, last_result: { status: "unsupported-command", execution_id: null, accepted_at: null } };
  const graph = fixtureGraph();
  transport.graph = { ...graph, truncated: true, nodes: [{ ...graph.nodes[0], outcomes: [{ bucket: "command-outcome", handlers: [refused] }] }, ...graph.nodes.slice(1)] };
  const view = await mount(transport);
  try {
    assert.ok(view.host.querySelector('[role="alert"]')?.textContent?.includes("snapshot is incomplete"));
    await click(button(view.host, "Watcher A"));
    assert.ok(details(view.host).textContent?.includes(refused.id));
    assert.ok(details(view.host).textContent?.includes(refused.availability));
    assert.ok(details(view.host).textContent?.includes(refused.last_result.status));
    assert.equal(transport.stops.length, 0);
  } finally { await view.close(); }
});

test("measured graph fits once, then keeps the user's camera and exact selection across refresh, resize and list toggle", async () => {
  const view = await mount();
  try {
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 800, 480), "initial frame uses the measured viewport");
    const zoomIn = view.host.querySelector<HTMLButtonElement>('button[aria-label="Zoom in"]');
    assert.ok(zoomIn);
    await click(zoomIn);
    assert.notEqual(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 800, 480));
    await click(button(view.host, "Watcher A"));
    const camera = cameraLayer(view.host).style.transform;
    view.transport.graph = { ...fixtureGraph(), revision: "revision-b", nodes: [...fixtureGraph().nodes].reverse().map((node) => ({ ...node, state: "active" })) };
    await act(async () => { view.transport.changed!(); });
    assert.equal(cameraLayer(view.host).style.transform, camera, "data-only refresh and node reorder keep the camera");
    assert.equal(view.host.querySelector(`[aria-label^="sentinel: Watcher A"]`)?.getAttribute("aria-pressed"), "true", "selection follows the exact identity");
    assert.ok(details(view.host).textContent?.includes("active"));
    await resize(640, 420);
    assert.equal(cameraLayer(view.host).style.transform, camera, "resize never overrides a moved camera");
    await click(button(view.host, "Show list"));
    await click(button(view.host, "Show graph"));
    assert.equal(cameraLayer(view.host).style.transform, camera, "list toggle restores the same camera");
    await click(button(view.host, "Fit graph"));
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 640, 420, true));
    await resize(900, 500);
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 900, 500, true), "an untouched fitted camera follows the measured size");
  } finally { await resize(800, 480); await view.close(); }
});

test("removed selected participant shows an honest unavailable detail and recovers focus", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Show list"));
    await click(button(view.host, "Worker A"));
    assert.equal(document.activeElement, details(view.host));
    const graph = fixtureGraph();
    view.transport.graph = { ...graph, nodes: graph.nodes.filter((node) => node.identity.kind !== "worker"), edges: graph.edges.filter((edge) => edge.target.kind !== "worker") };
    await act(async () => { view.transport.changed!(); });
    const unavailable = view.host.querySelector<HTMLElement>('aside[aria-label="Details: unavailable participant"]');
    assert.ok(unavailable, "removal is explicit instead of silently closing details");
    assert.equal(document.activeElement, unavailable, "focus lands on the replacement detail rather than the document body");
    assert.ok(!view.host.querySelector('ul[aria-label="Automation participants"]')?.textContent?.includes("Worker A"));
    await act(async () => { unavailable.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    assert.equal(view.host.querySelector("aside"), null);
    assert.ok(view.host.querySelector('ul[aria-label="Automation participants"]')?.contains(document.activeElement), "focus falls back to a live participant");
  } finally { await view.close(); }
});

test("details Escape stays inside the graph and returning to Sentinels restores the opener entry", async () => {
  const view = await mount();
  let outer = 0;
  const listener = (event: KeyboardEvent) => { if (event.code === "Escape") outer++; };
  window.addEventListener("keydown", listener as never);
  try {
    const node = button(view.host, "Watcher A");
    await click(node);
    await act(async () => { details(view.host).dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    assert.equal(outer, 0, "Escape does not reach surrounding overlays");
    assert.equal(document.activeElement, node);
    await click(button(view.host, "All Sentinels"));
    assert.equal((document.activeElement as HTMLElement | null)?.dataset.scenarioId, view.transport.graph.scenarios[0].id);
  } finally {
    window.removeEventListener("keydown", listener as never);
    await view.close();
  }
});

test("authoritative event replaces the selected command result and execution without reselection", async () => {
  const transport = new FixtureTransport();
  const capture = { total_bytes: 3, retained_bytes: 3, truncated: false };
  const success: CommandResultMetadata = { status: "success", exit_code: 0, resources_confirmed: true, stdout: capture, stderr: capture, started_at: 100001, finished_at: 100010 };
  const withCommand = (execution_id: string, status: string, command_result: CommandResultMetadata): AutomationGraph => {
    const graph = fixtureGraph();
    const result = { status, execution_id, accepted_at: 100000, command_result };
    const handler = { id: "marker", type: "command", target: null, availability: "available" as const, label: "Write marker", timeout_ms: 1000, last_result: result };
    return { ...graph, revision: execution_id, nodes: graph.nodes.map((node) => node.identity.kind === "sentinel" ? { ...node, outcomes: [...node.outcomes, { bucket: "command-outcome", handlers: [handler] }], last_result: { bucket: "command-outcome", accepted_at: 100000, handlers: [{ ...result, handler_id: handler.id }] } } : node) };
  };
  const commandFacts = (host: HTMLElement) => {
    const list = Array.from(details(host).querySelectorAll('ul[aria-label="Last accepted handler results"] dl')).find((entry) => entry.getAttribute("aria-label") === commandResultLabel("marker"));
    assert.ok(list, "selected node renders its accepted command result");
    return { row: list.closest("li"), facts: new Map(Array.from(list.querySelectorAll("dt")).map((term) => [term.textContent, term.nextElementSibling?.textContent])) };
  };
  transport.graph = withCommand("command-execution-a", "completed", success);
  const view = await mount(transport);
  try {
    await click(button(view.host, "Watcher A"));
    const panel = details(view.host);
    assert.equal(commandFacts(view.host).facts.get(COMMAND_FACT.outcome), "success");
    assert.ok(commandFacts(view.host).row?.textContent?.includes("command-execution-a"));
    assert.equal(document.activeElement, panel);
    view.transport.graph = withCommand("command-execution-b", "failed", { ...success, status: "nonzero", exit_code: 7 });
    await act(async () => { view.transport.changed!(); });
    assert.equal(details(view.host), panel, "the same details region stays mounted");
    assert.equal(document.activeElement, panel, "focus is preserved across the authoritative refresh");
    assert.equal(view.host.querySelector(`[aria-label^="sentinel: Watcher A"]`)?.getAttribute("aria-pressed"), "true");
    const { row, facts } = commandFacts(view.host);
    assert.equal(facts.get(COMMAND_FACT.outcome), "nonzero");
    assert.equal(facts.get(COMMAND_FACT.exit), "7");
    assert.ok(row?.textContent?.includes("command-execution-b"));
    assert.ok(!details(view.host).textContent?.includes("command-execution-a"), "the previous execution is not retained");
    assert.equal(view.transport.reads, 2);
  } finally { await view.close(); }
});

test("measured 500px canvas opens the chain top to bottom, desktop width runs left to right, and a moved camera keeps its geometry", async () => {
  Object.assign(viewportSize, { width: 500, height: 576 });
  const transport = new FixtureTransport();
  transport.graph = chainGraph();
  const view = await mount(transport);
  try {
    const narrow = projectedCards(view.host, CHAIN);
    assert.ok(camera(view.host).scale >= READABLE_SCALE);
    readable(narrow, 500, 576);
    flows(narrow, "down");
    const labels = Array.from(view.host.querySelectorAll('[data-edge-kind] text')).map((text) => Array.from(text.querySelectorAll("tspan")).map((line) => line.textContent));
    assert.deepEqual(labels, transport.graph.edges.map((edge) => [edge.bucket, `${edge.handler_type} · ${edge.result!.status}`]), "branch labels stay whole");
    await resize(1400, 576);
    const wide = projectedCards(view.host, CHAIN);
    readable(wide, 1400, 576);
    flows(wide, "right");
    await click(view.host.querySelector<HTMLButtonElement>('button[aria-label="Zoom in"]')!);
    const moved = cameraLayer(view.host).style.transform;
    const boxes = CHAIN.map((name) => view.host.querySelector<HTMLElement>(`button[aria-label*=": ${name} · "]`)!.getAttribute("style"));
    await resize(500, 576);
    assert.equal(cameraLayer(view.host).style.transform, moved, "a narrower canvas never resets the moved camera");
    assert.deepEqual(CHAIN.map((name) => view.host.querySelector<HTMLElement>(`button[aria-label*=": ${name} · "]`)!.getAttribute("style")), boxes, "geometry under a moved camera is not re-oriented");
    await click(button(view.host, "Fit graph"));
    assert.equal(cameraLayer(view.host).style.transform, fitted(transport.graph, 500, 576, true), "explicit Fit uses the current measured canvas");
    flows(projectedCards(view.host, CHAIN), "down");
  } finally { await resize(800, 480); await view.close(); }
});

test("an unmeasured canvas paints no guessed camera until its real size is observed", async () => {
  Object.assign(viewportSize, { width: 0, height: 0 });
  const view = await mount();
  try {
    assert.equal(cameraLayer(view.host).style.transform, "", "no default-size transform before measurement");
    assert.equal(cameraLayer(view.host).style.visibility, "hidden");
    await resize(500, 576);
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 500, 576));
    assert.equal(cameraLayer(view.host).style.visibility, "");
  } finally { await resize(800, 480); await view.close(); }
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
