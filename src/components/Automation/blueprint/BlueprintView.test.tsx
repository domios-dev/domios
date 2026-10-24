import assert from "node:assert/strict";
import { act, Profiler } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { BlueprintView } from "./BlueprintView";
import { watchRuns, withRetainedRuns } from "./runEvidence";
import { call, diff, viewerPipeline } from "../blueprintViewer.testdata";
import { BlueprintProjectList } from "../../Jobs/BlueprintProjectList";
import { document as source } from "../blueprintViewer.testdata";
import capturedWatch from "../blueprintWatchCaptured.testdata.json";
import { pipelineModel } from "../pipelineModel";
import type { AutomationGraph } from "../types";

async function main() {
  const nativeMessageChannel = globalThis.MessageChannel;
  class TestMessageChannel {
    port1 = { onmessage: null as (() => void) | null, close() {} };
    port2 = { postMessage: () => queueMicrotask(() => this.port1.onmessage?.()), close() {} };
  }
  Object.defineProperty(globalThis, "MessageChannel", { configurable: true, value: TestMessageChannel });
  const window = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "HTMLElement", "Element", "Node", "navigator", "Event", "MouseEvent", "KeyboardEvent", "MutationObserver"]) Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div"); document.body.append(host);
  Object.defineProperty(window.HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 960 });
  Object.defineProperty(window.HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 440 });
  const root = createRoot(host);
  const button = (name: string) => {
    const element = [...host.querySelectorAll("button")].find((element) => (element.getAttribute("aria-label") ?? element.textContent) === name);
    assert(element, `Missing accessible control ${name}`); return element;
  };
  const waitForNode = (name: string) => new Promise<void>((resolve, reject) => {
    const check = () => [...host.querySelectorAll("button")].some((button) => button.getAttribute("aria-label") === name);
    if (check()) { resolve(); return; }
    const observer = new MutationObserver(() => { if (check()) { observer.disconnect(); clearTimeout(deadline); resolve(); } });
    const deadline = setTimeout(() => { observer.disconnect(); reject(new Error(`Missing ${name}`)); }, 2000);
    observer.observe(host, { childList: true, subtree: true });
  });
  const details = () => [...host.querySelectorAll('aside[role="region"]')].map((entry) => entry.getAttribute("aria-label"));
  const journal = () => host.querySelector('[aria-label="Selected run journal"]');
  let stopped = 0, historyReads = 0;
  const loadHistory = async () => { historyReads++; return { current_version: 2, history_truncated: false, versions: [{ version: 1, replaces: null, diff: null }, { version: 2, replaces: 1, diff }] }; };
  try {
    await act(async () => root.render(<BlueprintView pipeline={viewerPipeline()} initialRunId="past" onStop={async () => { stopped++; }} canStop loadHistory={loadHistory} backLabel="All jobs" onBack={() => {}} />));
    await waitForNode("Node probe");
    const nothingAboveCanvas = (moment: string) => {
      const parts = [...host.querySelector('section[aria-label="Blueprint view"]')!.children];
      assert.equal(parts.length, 2, `${moment}: only the header row and the canvas body`);
      assert(parts[0].classList.contains("ct-jobs-header"), `${moment}: the header row comes first`);
      assert(parts[1].hasAttribute("data-blueprint-body"), `${moment}: the canvas body follows the header directly`);
      assert(parts[1].querySelector(".bp-canvas"), `${moment}: the canvas stays mounted`);
    };
    nothingAboveCanvas("first render");
    assert(host.querySelector('.bp-canvas [aria-label="Blueprint info"]'), "notes, purpose and legend live in the canvas info card");
    assert(host.querySelector('.bp-canvas ul[aria-label="Node types"]'));
    const layer = () => host.querySelector<HTMLElement>('.bp-canvas > div[style*="transform-origin"]');
    assert(layer());
    const fitted = layer()!.style.transform;
    await act(async () => button("Zoom in").click());
    assert.notEqual(layer()!.style.transform, fitted, "zoom in changes the camera");
    const zoomed = layer()!.style.transform;
    await act(async () => button("Zoom out").click());
    assert.notEqual(layer()!.style.transform, zoomed, "zoom out changes the camera");
    await act(async () => button("Zoom in").click());
    await act(async () => button("Fit all").click());
    const whole = layer()!.style.transform;
    await act(async () => button("Zoom in").click());
    await act(async () => button("Fit all").click());
    assert.equal(layer()!.style.transform, whole, "Fit all frames the whole graph again");
    const callout = host.querySelector('[role="status"][aria-label="Recorded stop"]');
    assert(callout?.closest(".bp-canvas"), "the recorded stop is drawn on the canvas");
    assert(callout?.textContent?.includes("contract_violation"));
    assert(journal()?.querySelectorAll("li").length);
    assert(journal()?.textContent?.includes("contract_violation"), "the reason also stays in the journal");
    assert([...host.querySelectorAll(".bp-wire")].length > 0);
    await act(async () => button("Node invoke").click());
    assert.deepEqual(details(), ["Details: invoke"]);
    assert(host.querySelector('[aria-label="Callee mem:helper"]'));
    assert.equal(button("Expand callee").getAttribute("aria-expanded"), "false");
    await act(async () => button("Expand callee").click());
    await waitForNode("Node return-count in invoke");
    assert.equal(button("Collapse callee").getAttribute("aria-expanded"), "true");
    assert(host.querySelector(".bp-frame")?.textContent?.includes("mem:helper"), "the callee opens in its own frame");
    const calloutNow = host.querySelector('[aria-label="Recorded stop"]');
    assert(calloutNow?.textContent?.includes("contract_violation"), "the stop follows the failing callee node once it is visible");
    await act(async () => button("Node return-count in invoke").click());
    assert.deepEqual(details(), ["Details: return-count in invoke"], "a click on another node switches details at once");
    assert(host.querySelector('[aria-label="Input port"]'));
    assert(host.querySelector('[aria-label="Output port"]'));
    assert(host.querySelector('aside [aria-label="Run evidence"]')?.textContent?.includes("contract_violation"), "the reason stays in node details");
    await act(async () => button("Semantic diff").click());
    assert.equal(historyReads, 1);
    assert(host.querySelector('aside[aria-label="Semantic diff"] table caption'), "the diff opens in the side drawer, not above the canvas");
    nothingAboveCanvas("diff open");
    const select = host.querySelector<HTMLSelectElement>('[aria-label="Definition version"]'); assert(select);
    await act(async () => { select.value = "1"; select.dispatchEvent(new Event("change", { bubbles: true })); });
    assert(!host.querySelector("table"));
    await act(async () => button("Latest run").click());
    nothingAboveCanvas("switching to the latest run");
    assert(!host.querySelector('[aria-label="Recorded stop"]'));
    await act(async () => button("Run past").click());
    nothingAboveCanvas("switching back to a past run");
    await waitForNode("Node probe");
    nothingAboveCanvas("after the past run graph is built");
    await act(async () => button("Latest run").click());
    await act(async () => button("Stop Blueprint").click());
    assert.equal(stopped, 1);

    const watch = capturedWatch as AutomationGraph;
    const runs = watchRuns(watch, watch.blueprints![0]);
    const watchPipeline = withRetainedRuns(pipelineModel(watch.blueprints![0]), runs);
    await act(async () => root.render(<BlueprintView key="watch" pipeline={watchPipeline} initialRunId={runs[0].id} />));
    await waitForNode("Node decide");
    assert(button(`Run ${runs[0].id}`));
    assert(journal()?.textContent?.includes("passed"));
    assert(journal()?.textContent?.includes("done"));
    for (const name of ["Node source", "Node decide", "Node notify", "Fit all", "Zoom in", "Zoom out"]) assert(button(name), name);
    assert(button("Node decide").textContent?.includes("passed"), "the retained outcome reaches the typed node");
    assert(!host.querySelector('.bp-node[data-state="waiting"]'));

    await act(async () => root.render(<BlueprintView key="held" pipeline={{ ...viewerPipeline(), state: "held", heldReason: "uncertain_pipeline_claim" }} />));
    assert([...host.querySelectorAll('[role="status"]')].some((entry) => entry.textContent?.includes("Held")));
    const unavailable = viewerPipeline();
    unavailable.runs = unavailable.runs?.map((run) => ({ ...run, pipeline: { ...run.pipeline, historical_graph_available: false, call_graph: undefined, expanded_calls: [] } }));
    await act(async () => root.render(<BlueprintView key="unavailable" pipeline={unavailable} initialRunId="past" />));
    await act(async () => button("Run past").click());
    assert(host.querySelector('[aria-label="Historical graph unavailable"]'));
    assert(journal());
    assert(host.querySelector('[aria-label="Recorded stop"]')?.textContent?.includes("contract_violation"), "without a graph the stop stays above the journal");
    assert(!host.querySelector('[aria-label="Node invoke"]'));

    const calls: string[] = [];
    await act(async () => root.render(<BlueprintProjectList workspaceId="workspace" api={{ list: async (workspace) => { calls.push(workspace); return [{ path: ".domios/blueprints/viewer-fixture.yaml", name: source.name, intent: source.intent ?? null, error: null }, { path: ".domios/blueprints/bad.yaml", name: null, intent: null, error: "mismatch" }]; }, show: async (_, path) => { calls.push(path); return { path, document: source }; } }} />));
    assert.equal(calls.length, 0);
    await act(async () => button("List repository Blueprints").click());
    assert.deepEqual(calls, ["workspace"]);
    assert(!host.querySelector('[aria-label="View .domios/blueprints/bad.yaml"]'));
    await act(async () => button("View .domios/blueprints/viewer-fixture.yaml").click());
    assert(host.querySelector('[aria-label="Repository source .domios/blueprints/viewer-fixture.yaml"]'));
    assert(![...host.querySelectorAll("button")].some((button) => /^(apply|run|approve|stop blueprint)$/i.test(button.getAttribute("aria-label") ?? button.textContent ?? "")), "a repository source is read-only");

    const large = { ...viewerPipeline(), runs: [], nested_run: null, runId: null,
      call_graph: { ...source, steps: Array.from({ length: 24 }, (_, index) => ({ ...source.steps[2], id: `call-${index}`, routes: { done: "@next", failed: "@stop" } })) },
      expanded_calls: Array.from({ length: 24 }, (_, index) => ({ ...call, node: `call-${index}` })),
    };
    let maxRender = 0, inputLatency = -1, scrollLatency = -1, sentAt = 0;
    await act(async () => root.render(<Profiler id="large-viewer" onRender={(_, __, duration) => { maxRender = Math.max(maxRender, duration); }}><input aria-label="Responsiveness probe" onInput={() => { inputLatency = performance.now() - sentAt; }} /><div aria-label="Scroll probe" onScroll={() => { scrollLatency = performance.now() - sentAt; }}><BlueprintView pipeline={large} /></div></Profiler>));
    await act(async () => { sentAt = performance.now(); host.querySelector<HTMLInputElement>('[aria-label="Responsiveness probe"]')!.dispatchEvent(new Event("input", { bubbles: true })); sentAt = performance.now(); host.querySelector('[aria-label="Scroll probe"]')!.dispatchEvent(new Event("scroll")); });
    assert(inputLatency >= 0 && scrollLatency >= 0);
    await waitForNode("Node call-23");
    assert(!host.querySelector(".bp-node[data-state]"), "a definition without a run shows no per-node states");
    await act(async () => button("Node call-0").click());
    await act(async () => button("Expand callee").click());
    await waitForNode("Node return-count in call-0");
    await act(async () => button("Node call-17").click());
    assert.deepEqual(details(), ["Details: call-17"], "the clicked node opens, never a neighbour");
    assert.equal(button("Node call-17").getAttribute("aria-pressed"), "true");

    const leaf = { schema_version: 2 as const, kind: "pipeline" as const, name: "leaf", steps: [{ id: "deepest", run: { lit: "printf leaf" } }] };
    const middle = { schema_version: 2 as const, kind: "pipeline" as const, name: "middle", steps: [{ id: "inner", call: { ref: "mem:leaf", pin: { id: "mem:leaf", hash: "c".repeat(64) } } }] };
    const nested = { ...viewerPipeline(), runs: [], nested_run: null, runId: null,
      expanded_calls: [{ ...call, graph: middle, children: [{ node: "inner", pin: { id: "mem:leaf", hash: "c".repeat(64) }, graph: leaf, children: [] }] }] };
    const dispatch = (element: Element, event: Event) => act(async () => { element.dispatchEvent(event); });
    let left = 0;
    await act(async () => root.render(<BlueprintView key="nested" pipeline={nested} backLabel="All jobs" onBack={() => { left++; }} />));
    await waitForNode("Node invoke");
    await dispatch(button("Node invoke"), new MouseEvent("dblclick", { bubbles: true }));
    await waitForNode("Node inner in invoke");
    assert(!host.querySelector('[aria-label="Node probe"]'), "drilling in shows only the callee graph");
    assert.equal(host.querySelector('nav[aria-label="Blueprint path"] [aria-current="page"]')?.textContent, "mem:helper");
    await dispatch(button("Node inner in invoke"), new KeyboardEvent("keydown", { code: "Enter", bubbles: true, cancelable: true }));
    await waitForNode("Node deepest in invoke / inner");
    assert.deepEqual([...host.querySelectorAll('nav[aria-label="Blueprint path"] li')].map((entry) => entry.textContent), ["viewer-fixture", "mem:helper", "mem:leaf"]);
    await act(async () => button("mem:helper").click());
    await waitForNode("Node inner in invoke");
    assert(!host.querySelector('[aria-label="Node deepest in invoke / inner"]'), "the breadcrumb steps back one level");
    await dispatch(button("Node inner in invoke"), new KeyboardEvent("keydown", { code: "Escape", bubbles: true, cancelable: true }));
    await waitForNode("Node probe");
    assert.equal(host.querySelector('nav[aria-label="Blueprint path"]'), null, "Escape returns to the parent graph");
    assert.equal(left, 0, "stepping out of a callee never leaves the Blueprint");
    await dispatch(button("Node invoke"), new MouseEvent("dblclick", { bubbles: true }));
    await waitForNode("Node inner in invoke");
    await act(async () => button("Back to viewer-fixture").click());
    await waitForNode("Node probe");
    assert.equal(left, 0);

    const failed = viewerPipeline();
    failed.state = "failed";
    failed.runs = [];
    failed.nodes = [{ ...failed.nodes[0], key: "step-0", kind: "step", state: "failed", reason: "deadline", resultSummary: "deadline" }];
    failed.nested_run = { id: "live", nodes: [{ step_index: 0, step_name: "probe", state: "done", outcome: "unchanged" }] };
    await act(async () => root.render(<BlueprintView key="failed" pipeline={failed} />));
    await waitForNode("Node probe");
    assert(button("Node probe").textContent?.includes("failed"));
    assert(button("Node probe").textContent?.includes("deadline"));
    assert(journal()?.textContent?.includes("failed"));
    assert(journal()?.textContent?.includes("deadline"));
    assert(host.querySelector('[aria-label="Recorded stop"]')?.textContent?.includes("deadline"), "a deadline is a visible failure on the canvas");
    assert(!button("Node Stop").textContent?.includes("Target unavailable"));
    console.log(`BlueprintView: accessible interactions passed; 24 root calls + expanded callee; headless maxReactRender=${maxRender.toFixed(3)}ms inputDispatch=${inputLatency.toFixed(3)}ms scrollDispatch=${scrollLatency.toFixed(3)}ms`);
  } finally { await act(async () => root.unmount()); host.remove(); await window.happyDOM.close(); Object.defineProperty(globalThis, "MessageChannel", { configurable: true, value: nativeMessageChannel }); }
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
