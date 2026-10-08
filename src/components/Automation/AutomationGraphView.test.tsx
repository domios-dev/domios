import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { AutomationGraphView } from "./AutomationGraphView";
import { attachment, chainGraph, decisionGraph, fixtureGraph, FixtureTransport, MODEL_CARDS, modelGraph, multiScenarioGraph, sentinelIdentity } from "./fixtures.testdata";
import { automationKey, type AutomationGraph } from "./types";
import { NODE_WIDTH, planAutomationView, READABLE_SCALE } from "./layout";
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
function fitted(graph: AutomationGraph, width: number, height: number, whole = false, focus: string | null = null) {
  const fit = planAutomationView(scopedAutomationGraph(graph, attachment), { width, height }, whole, focus).transform;
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
/** The world point under the canvas center plus scale: what a moved camera keeps when the canvas resizes. */
function centered(host: HTMLElement, width: number, height: number) {
  const t = camera(host);
  return { x: (width / 2 - t.x) / t.scale, y: (height / 2 - t.y) / t.scale, scale: t.scale };
}
function sameCenter(a: ReturnType<typeof centered>, b: ReturnType<typeof centered>, message: string) {
  for (const axis of ["x", "y", "scale"] as const) assert.ok(Math.abs(a[axis] - b[axis]) < 1e-6, `${message}: ${axis} ${a[axis]} vs ${b[axis]}`);
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

test("graph native buttons provide keyboard selection and detail Escape restores focus", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Runs"));
    assert.ok(view.host.querySelector('section[aria-label="Blueprint runs"]'));
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

test("one named Sentinel entry opens its internal stages while shared execution keeps exact evidence", async () => {
  const transport = new FixtureTransport();
  transport.graph = multiScenarioGraph();
  const view = await mount(transport);
  try {
    assert.ok(view.host.textContent?.includes("Internal stage A"));
    assert.equal(view.host.querySelector("h2")?.textContent, `${transport.graph.scenarios[0].name} · Blueprint`);
    assert.ok(!view.host.textContent?.includes("Watcher B"));
    assert.equal(view.host.querySelectorAll('[data-edge-kind="observed"]').length, 1);
    await click(button(view.host, "All Blueprints"));
    assert.equal(view.host.querySelectorAll('ul[aria-label="Blueprint scenarios"] > li').length, 2);
    assert.ok(!view.host.textContent?.includes("Internal stage A"), "internal stage is not a separate scenario entry");
    await click(button(view.host, "Blueprint scenario B"));
    assert.equal(view.host.querySelector("h2")?.textContent, `${transport.graph.scenarios[1].name} · Blueprint`);
    assert.ok(view.host.textContent?.includes("Watcher B"));
    assert.ok(!view.host.textContent?.includes("Watcher A"));
    assert.ok(!view.host.textContent?.includes("Internal stage A"));
    assert.equal(view.host.querySelectorAll('[data-edge-kind="observed"]').length, 1);
    await click(button(view.host, "Schedule A"));
    assert.equal(details(view.host).getAttribute("aria-label"), "Details: Schedule A");
    transport.graph = { ...transport.graph, scenarios: [transport.graph.scenarios[0]] };
    await act(async () => { transport.changed!(); });
    assert.ok(view.host.querySelector('[role="alert"]')?.textContent?.includes("selected Blueprint is unavailable"));
    assert.equal(view.host.querySelector("aside"), null);
    assert.equal(view.host.querySelectorAll('[data-edge-kind]').length, 0);
  } finally { await view.close(); }
});

test("opening a panel refits the graph, then keeps the user's camera and exact selection across refresh, resize and Runs toggle", async () => {
  const view = await mount();
  try {
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 800, 480), "initial frame uses the measured viewport");
    const zoomIn = view.host.querySelector<HTMLButtonElement>('button[aria-label="Zoom in"]');
    assert.ok(zoomIn);
    await click(zoomIn);
    assert.notEqual(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 800, 480));
    const beforeDrawer = cameraLayer(view.host).style.transform;
    const region = view.host.querySelector('[aria-label="Automation graph"]');
    await click(button(view.host, "Watcher A"));
    const camera = cameraLayer(view.host).style.transform;
    assert.notEqual(camera, beforeDrawer, "opening the drawer refits the graph");
    assert.equal(camera, fitted(view.transport.graph, 800, 480, true), "opening a panel frames the whole visible graph");
    await click(zoomIn);
    const movedCamera = cameraLayer(view.host).style.transform;
    assert.equal(view.host.querySelector('[aria-label="Automation graph"]'), region, "the graph stays mounted behind the drawer");
    view.transport.graph = { ...fixtureGraph(), revision: "revision-b", nodes: [...fixtureGraph().nodes].reverse().map((node) => ({ ...node, state: "active" })) };
    await act(async () => { view.transport.changed!(); });
    assert.equal(cameraLayer(view.host).style.transform, movedCamera, "data-only refresh and node reorder keep the camera");
    assert.equal(view.host.querySelector(`[aria-label^="Logic node: Watcher A"]`)?.getAttribute("aria-pressed"), "true", "selection follows the exact identity");
    assert.equal(details(view.host).getAttribute("aria-label"), "Details: Watcher A");
    const center = centered(view.host, 800, 480);
    await resize(640, 420);
    sameCenter(centered(view.host, 640, 420), center, "resize keeps a moved camera's scale and center");
    const resized = cameraLayer(view.host).style.transform;
    await click(button(view.host, "Runs"));
    await click(button(view.host, "Runs"));
    assert.equal(cameraLayer(view.host).style.transform, resized, "list toggle restores the same camera");
    await click(button(view.host, "Runs"));
    await resize(720, 460);
    await click(button(view.host, "Runs"));
    sameCenter(centered(view.host, 720, 460), center, "a restored camera adapts to the size it reopens at");
    await resize(640, 420);
    await click(button(view.host, "Fit graph"));
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 640, 420, false, automationKey(sentinelIdentity)), "Fit frames the selected participant readably");
    await resize(900, 500);
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 900, 500, false, automationKey(sentinelIdentity)), "an untouched fitted camera follows the measured size");

  } finally { await resize(800, 480); await view.close(); }
});

test("removed selected participant shows an honest unavailable detail and recovers focus", async () => {
  const view = await mount();
  try {
    await click(button(view.host, "Runs"));
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
    assert.equal(document.activeElement?.getAttribute("aria-label"), "Automation graph");
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
    await click(button(view.host, "All Blueprints"));
    assert.equal((document.activeElement as HTMLElement | null)?.dataset.scenarioId, view.transport.graph.scenarios[0].id);
  } finally {
    window.removeEventListener("keydown", listener as never);
    await view.close();
  }
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
    const moved = centered(view.host, 1400, 576);
    const boxes = CHAIN.map((name) => view.host.querySelector<HTMLElement>(`button[aria-label*=": ${name} · "]`)!.getAttribute("style"));
    await resize(500, 576);
    sameCenter(centered(view.host, 500, 576), moved, "a narrower canvas never resets the moved camera");
    assert.deepEqual(CHAIN.map((name) => view.host.querySelector<HTMLElement>(`button[aria-label*=": ${name} · "]`)!.getAttribute("style")), boxes, "geometry under a moved camera is not re-oriented");
    await click(button(view.host, "Fit graph"));
    assert.equal(cameraLayer(view.host).style.transform, fitted(transport.graph, 500, 576), "explicit Fit uses the current measured canvas");
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

test("node details open as one right overlay drawer that switches in place and returns focus behind it", async () => {
  const view = await mount();
  try {
    const region = view.host.querySelector('[aria-label="Automation graph"]')!;
    const opener = button(view.host, "Watcher A");
    await click(opener);
    const drawer = details(view.host);
    assert.ok(drawer.parentElement?.classList.contains("plan-side-panel-layer"), "details render inside the shared right overlay layer");
    assert.ok(!region.contains(drawer) && drawer.compareDocumentPosition(region) & Node.DOCUMENT_POSITION_PRECEDING, "graph stays in place before the overlay");
    assert.equal(drawer.getAttribute("aria-label"), "Details: Watcher A");
    assert.equal(drawer.getAttribute("aria-modal"), null, "the drawer is not modal; the graph behind stays usable");
    assert.equal(document.activeElement, drawer);
    await click(view.host.querySelector<HTMLButtonElement>('[aria-label="Automation graph"] button[aria-label^="Schedule node: Schedule A"]')!);
    assert.equal(details(view.host), drawer, "selecting another graph node updates the same drawer");
    assert.equal(drawer.getAttribute("aria-label"), "Details: Schedule A");
    await click(button(view.host, "Worker A"));
    assert.equal(details(view.host), drawer, "a linked participant opens in the same drawer");
    assert.equal(drawer.getAttribute("aria-label"), "Details: Worker A");
    await act(async () => { drawer.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    assert.equal(view.host.querySelector("aside"), null);
    assert.ok(region.contains(document.activeElement), "focus returns to the graph opener rather than a link inside the closed drawer");
  } finally { await view.close(); }
});

test("declared decision outcomes fan out to action leaves with taken and untaken paths and open their own inspector", async () => {
  const transport = new FixtureTransport();
  transport.graph = decisionGraph();
  const view = await mount(transport);
  try {
    const leaves = Array.from(view.host.querySelectorAll<HTMLButtonElement>("[data-action-key]"));
    const declared = transport.graph.nodes.flatMap((node) => node.outcomes.flatMap((outcome) => outcome.handlers.length ? outcome.handlers.filter((handler) => !handler.target) : [null]));
    assert.equal(leaves.length, declared.length, "every target-less handler and empty outcome has its own leaf");
    for (const handler of declared.filter((entry) => entry !== null)) {
      const leaf = leaves.find((entry) => entry.textContent?.includes(handler.label ?? handler.id));
      assert.ok(leaf, `${handler.id} leaf shows its safe label`);
    }
    assert.equal(view.host.querySelectorAll('[data-declared-edge="taken"]').length, 1, "only the accepted command path is solid");
    assert.equal(view.host.querySelectorAll('[data-declared-edge="untaken"]').length, declared.length - 1);
    assert.equal(view.host.querySelectorAll("[data-edge-kind]").length, transport.graph.edges.length, "real route evidence is unchanged");
    assert.equal(view.host.querySelectorAll('[aria-label="Automation graph"] button[aria-pressed]').length, transport.graph.nodes.length, "no participant is invented");
    const notify = leaves.find((entry) => entry.textContent?.includes("notify-d"))!;
    await click(notify);
    assert.equal(details(view.host).getAttribute("aria-label"), "Details: notify-d", "an action leaf opens its own node");
    assert.ok(details(view.host).textContent?.includes("notify-d"));
  } finally { await view.close(); }
});

function allCards(host: HTMLElement) {
  return Array.from(host.querySelectorAll<HTMLButtonElement>('[aria-label="Automation graph"] > div > button'));
}
async function press(host: HTMLElement, code: string, times = 1) {
  const region = host.querySelector<HTMLElement>('[aria-label="Automation graph"]')!;
  for (let count = 0; count < times; count++) await act(async () => { region.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true })); });
}

test("the observed seven-card fanout opens readable at 500x782 and Fit always returns a readable view", async () => {
  Object.assign(viewportSize, { width: 500, height: 782 });
  const transport = new FixtureTransport();
  transport.graph = modelGraph();
  const view = await mount(transport);
  try {
    assert.equal(allCards(view.host).length, MODEL_CARDS.length, "no branch is hidden");
    readable(projectedCards(view.host, MODEL_CARDS), 500, 782);
    await press(view.host, "Minus", 4);
    await press(view.host, "ArrowLeft", 5);
    assert.ok(camera(view.host).scale < READABLE_SCALE, "the user zoomed out on purpose");
    await click(button(view.host, "Fit graph"));
    assert.equal(cameraLayer(view.host).style.transform, fitted(transport.graph, 500, 782));
    readable(projectedCards(view.host, MODEL_CARDS), 500, 782);
    await resize(500, 600);
    assert.ok(camera(view.host).scale >= READABLE_SCALE, "a shorter canvas keeps the readable lower bound");
    assert.ok(projectedCards(view.host, ["Model parent"]).every((card) => card.x >= 0 && card.y >= 0 && card.x + card.width <= 500 && card.y + card.height <= 600), "the root stays framed");
    await click(button(view.host, "Runs"));
    await click(button(view.host, "Runs"));
    assert.equal(cameraLayer(view.host).style.transform, fitted(transport.graph, 500, 600), "list toggle keeps the readable frame");
    await click(view.host.querySelector<HTMLButtonElement>('[aria-label="Automation graph"] button[aria-label^="Logic node: Model child"]')!);
    await act(async () => { details(view.host).dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
    await click(view.host.querySelector<HTMLButtonElement>('[aria-label="Automation graph"] button[aria-label^="Logic node: Model child"]')!);
    await press(view.host, "ArrowUp", 12);
    await click(button(view.host, "Fit graph"));
    readable(projectedCards(view.host, ["Model child"]), 500, 600);

    await click(button(view.host, "Fit graph"));
    assert.ok(camera(view.host).scale >= READABLE_SCALE, "Fit is the readable return from the overview");
    for (const card of allCards(view.host)) {
      await act(async () => { card.focus(); });
      const t = camera(view.host);
      const [left, top, width, height] = [card.style.left, card.style.top, card.style.width, card.style.height].map((value) => Number.parseFloat(value));
      const box = { x: left * t.scale + t.x, y: top * t.scale + t.y, width: width * t.scale, height: height * t.scale };
      assert.ok(box.x >= 0 && box.y >= 0 && box.x + box.width <= 500 && box.y + box.height <= 600, `${card.getAttribute("aria-label")} is reachable by keyboard focus`);
      assert.ok(t.scale >= READABLE_SCALE);
    }
  } finally { await resize(800, 480); await view.close(); }
});

test("Runs toggles the log without replacing the graph and Blueprint stop stays in the toolbar", async () => {
  const view = await mount();
  try {
    const graph = view.host.querySelector('[aria-label="Automation graph"]');
    assert.equal(view.host.querySelector('section[aria-label="Blueprint runs"]'), null);
    assert.ok(!Array.from(view.host.querySelectorAll("button")).some((entry) => ["Overview", "Show list"].includes(entry.textContent ?? "")));
    await click(button(view.host, "Runs"));
    assert.ok(view.host.querySelector('section[aria-label="Blueprint runs"]'));
    assert.equal(view.host.querySelector('[aria-label="Automation graph"]'), graph);
    await click(button(view.host, "Runs"));
    assert.equal(view.host.querySelector('section[aria-label="Blueprint runs"]'), null);
    await click(button(view.host, "Watcher A"));
    assert.equal(details(view.host).querySelector('button[aria-label="Stop Blueprint"]'), null);
    await click(button(view.host, "Stop Blueprint"));
    assert.deepEqual(view.transport.stops, [sentinelIdentity]);
    assert.equal(button(view.host, "Stopping").disabled, true);
    await act(async () => { view.transport.stopResult.reject(new Error("private-error")); });
    assert.ok(view.host.querySelector('[role="alert"]'));
    assert.ok(!view.host.textContent?.includes("private-error"));
  } finally { await view.close(); }
});

test("an open node panel refits into the remaining measured canvas width", async () => {
  const view = await mount();
  try {
    await press(view.host, "Minus", 3);
    await click(button(view.host, "Watcher A"));
    await resize(440, 480);
    assert.equal(cameraLayer(view.host).style.transform, fitted(view.transport.graph, 440, 480, true));
    const cards = allCards(view.host);
    assert.ok(cards.length > 0);
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
