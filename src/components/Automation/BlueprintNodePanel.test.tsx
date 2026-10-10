import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { BlueprintNodeDetails } from "./BlueprintNodeDetails";
import { blueprintModel, nodeKeys } from "./blueprintModel";
import { typedGraph, watchIdentity } from "./blueprintModel.testdata";
import { runRow, type ReplayStep } from "./blueprintRunModel";
import { RunDivider } from "./RunDivider";
import { RunLog } from "./RunLog";
import { graphRuns } from "./blueprintRunModel";
import { automationKey, type AutomationGraph } from "./types";
import { useBlueprintReplay } from "./useBlueprintReplay";

async function main() {
const window = new Window({ url: "http://localhost" });
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "navigator", "KeyboardEvent", "PointerEvent"]) Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
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
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
const click = async (button: HTMLElement) => act(async () => { button.focus(); button.click(); });
const key = async (target: HTMLElement, code: string) => act(async () => { target.dispatchEvent(new window.KeyboardEvent("keydown", { code, bubbles: true, cancelable: true }) as unknown as Event); });
const watch = automationKey(watchIdentity);

try {
  const graph = typedGraph();
  const model = blueprintModel(graph);
  const node = (key: string) => model.nodes.find((entry) => entry.key === key)!;

  await act(async () => root.render(<BlueprintNodeDetails node={node(nodeKeys(watch).decision)} graph={graph} onClose={() => {}} />));
  assert.ok(host.querySelector('section[aria-label="Instruction"]'), "agent drawer shows its instruction");
  assert.equal(host.querySelector('section[aria-label="Routes"] dl')?.children.length, 3);
  assert.equal(host.querySelector('section[aria-label="Node code"]'), null, "agent drawer never shows code");

  let closed = 0;
  await act(async () => root.render(<BlueprintNodeDetails node={node(nodeKeys(watch).source)} graph={graph} onClose={() => closed++} />));
  const code = host.querySelector('section[aria-label="Node code"] code');
  assert.ok(code?.textContent?.includes("cargo check"), "source drawer shows its probe");
  const fullscreen = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent === "Fullscreen");
  assert.ok(fullscreen);
  await click(fullscreen);
  assert.ok(document.querySelector('[role="dialog"][aria-label="Fullscreen code"] code'));
  await act(async () => { window.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Escape", bubbles: true })); });
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(closed, 0, "fullscreen Escape leaves the node drawer open");
  await click(host.querySelector<HTMLButtonElement>('button[aria-label="Close details"]')!);
  assert.equal(closed, 1, "the drawer has its own close");

  const run = { id: "run-1", root_node: watchIdentity.id, root_transition: "t1", started_at: 1_000, parent: null };
  const ran: AutomationGraph = { ...graph, nodes: graph.nodes.map((entry, index) => index ? entry : { ...entry, history: [
    { id: "t1", bucket: "red", accepted_at: 1_000, causal_run: run, handlers: [{ handler_id: "notify-owner", status: "completed", execution_id: "a", accepted_at: 1_000 }] },
    { id: "t0", bucket: "green", accepted_at: 500, handlers: [] },
  ] }), runs: [{ id: "run-1", root_node: watchIdentity.id, root_transition: "t1", started_at: 1_000, state: "completed", incomplete: false, transitions: [{ node: watchIdentity, transition_id: "t1", parent: null }] }] };
  const rows = graphRuns(ran).map((entry) => runRow(model, entry));
  const replayed: string[] = [];
  await act(async () => root.render(<RunLog rows={rows} model={model} selected={null} onReplay={(row) => replayed.push(row.key)} />));
  const replays = Array.from(host.querySelectorAll<HTMLButtonElement>("button[data-run-replay]"));
  assert.equal(replays.length, 2);
  assert.match(replays[0].getAttribute("aria-label") ?? "", /^Replay run /);
  assert.deepEqual(replays.map((entry) => entry.tabIndex), [0, -1], "one roving stop for the list");
  await click(replays[0]);
  assert.deepEqual(replayed, ["run-1"]);
  await key(replays[0], "ArrowDown");
  assert.equal(document.activeElement, replays[1], "arrow keys move between runs");
  const steps = host.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")[0];
  await click(steps);
  assert.equal(steps.getAttribute("aria-expanded"), "true");
  assert.ok(document.getElementById(steps.getAttribute("aria-controls")!)?.textContent?.includes("Notify owner"), "the expanded timeline lists the action");
  await act(async () => root.render(<RunLog rows={[]} model={model} selected={null} onReplay={() => {}} />));
  assert.ok(host.querySelector('[role="status"]')?.textContent?.startsWith("No runs yet"));

  const sizes: number[] = [];
  await act(async () => root.render(<RunDivider height={260} available={800} controls="log" onResize={(height) => sizes.push(height)} />));
  const divider = host.querySelector<HTMLElement>('[role="separator"][aria-label="Resize run log"]')!;
  assert.equal(divider.getAttribute("aria-valuenow"), "260");
  assert.equal(divider.tabIndex, 0, "the divider is keyboard reachable");
  await key(divider, "ArrowUp");
  assert.deepEqual(sizes, [276]);
  await act(async () => {
    divider.dispatchEvent(new window.PointerEvent("pointerdown", { pointerId: 1, button: 0, clientY: 500, bubbles: true }) as unknown as Event);
    divider.dispatchEvent(new window.PointerEvent("pointermove", { pointerId: 1, clientY: 440, bubbles: true }) as unknown as Event);
    divider.dispatchEvent(new window.PointerEvent("pointerup", { pointerId: 1, clientY: 440, bubbles: true }) as unknown as Event);
  });
  assert.deepEqual(sizes, [276, 320], "dragging up grows the log");

  const replaySteps: ReplayStep[] = [{ nodes: ["a"], wires: [] }, { nodes: ["b"], wires: ["ab"] }];
  function Replay() {
    const state = useBlueprintReplay(replaySteps, 0);
    return <output>{state?.done ? "done" : state?.travelling.join(",") || "start"}</output>;
  }
  await act(async () => root.render(<Replay />));
  assert.ok(frame);
  await act(async () => { frame?.(0); });
  await act(async () => { frame?.(600); });
  assert.equal(host.querySelector("output")?.textContent, "ab");
  await act(async () => { frame?.(1200); });
  assert.equal(host.querySelector("output")?.textContent, "done");
  await act(async () => root.render(<div />));
  reduced = true;
  const before = scheduled;
  await act(async () => root.render(<Replay />));
  assert.equal(host.querySelector("output")?.textContent, "done");
  assert.equal(scheduled, before, "reduced motion schedules no travel frames");
} finally {
  await act(async () => root.unmount());
  host.remove();
}
console.log("Node drawer, run log rows, divider and replay clock wiring passed");
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
