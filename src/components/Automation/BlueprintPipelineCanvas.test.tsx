import assert from "node:assert/strict";
import { act, Profiler } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { BlueprintPipelineCanvas } from "./BlueprintPipelineCanvas";
import { call, diff, viewerPipeline } from "./blueprintViewer.testdata";
import { BlueprintProjectList } from "../Jobs/BlueprintProjectList";
import { document as source } from "./blueprintViewer.testdata";

async function main() {
  const nativeMessageChannel = globalThis.MessageChannel;
  class TestMessageChannel {
    port1 = { onmessage: null as (() => void) | null, close() {} };
    port2 = { postMessage: () => queueMicrotask(() => this.port1.onmessage?.()), close() {} };
  }
  Object.defineProperty(globalThis, "MessageChannel", { configurable: true, value: TestMessageChannel });
  const window = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "HTMLElement", "Element", "Node", "navigator", "Event", "MutationObserver"]) Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div"); document.body.append(host);
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
  let stopped = 0, historyReads = 0;
  const loadHistory = async () => { historyReads++; return { current_version: 2, history_truncated: false, versions: [{ version: 1, replaces: null, diff: null }, { version: 2, replaces: 1, diff }] }; };
  try {
    await act(async () => root.render(<BlueprintPipelineCanvas pipeline={viewerPipeline()} initialRunId="past" onStop={async () => { stopped++; }} loadHistory={loadHistory} />));
    assert(button("Stop selected run").disabled);
    assert(host.querySelector('[aria-label="Recorded stop"]'));
    assert(host.querySelector('[aria-label="Selected run journal"]'));
    assert(host.querySelectorAll('[aria-label="Selected run journal"] li').length > 0);
    const wires = [...host.querySelectorAll('svg[aria-hidden="true"] path')].filter((path) => path.getAttribute("stroke"));
    assert(wires.length > 0 && wires.every((path) => path.getAttribute("fill") === "none"), "wires must not be filled");
    assert(host.querySelector('[role="group"][aria-label="Run actions"]'));
    await act(async () => button("Node invoke").click());
    assert(host.querySelector('[aria-label="Callee mem:helper"]'));
    assert.equal(button("Expand callee").getAttribute("aria-expanded"), "false");
    await act(async () => button("Expand callee").click());
    await waitForNode("Node return-count in invoke");
    assert.equal(button("Collapse callee").getAttribute("aria-expanded"), "true");
    await act(async () => button("Node return-count in invoke").click());
    assert(host.querySelector('[aria-label="Input port"]'));
    assert(host.querySelector('[aria-label="Output port"]'));
    await act(async () => button("Semantic diff").click());
    assert.equal(historyReads, 1);
    assert(host.querySelector('table caption'));
    const select = host.querySelector<HTMLSelectElement>('[aria-label="Definition version"]'); assert(select);
    await act(async () => { select.value = "1"; select.dispatchEvent(new Event("change", { bubbles: true })); });
    assert(!host.querySelector('table'));
    await act(async () => button("Latest run").click());
    assert(!button("Stop selected run").disabled);
    assert(!host.querySelector('[aria-label="Recorded stop"]'));
    await act(async () => button("Stop selected run").click());
    assert.equal(stopped, 1);
    await act(async () => root.render(<BlueprintPipelineCanvas pipeline={{ ...viewerPipeline(), state: "held", heldReason: "uncertain_pipeline_claim" }} />));
    assert(host.querySelector('[role="status"]')?.textContent?.includes("Held"));
    const unavailable = viewerPipeline();
    unavailable.runs = unavailable.runs?.map((run) => ({ ...run, pipeline: { ...run.pipeline, historical_graph_available: false, call_graph: undefined, expanded_calls: [] } }));
    await act(async () => root.render(<BlueprintPipelineCanvas pipeline={unavailable} initialRunId="past" />));
    await act(async () => button("Run past").click());
    assert(host.querySelector('[aria-label="Historical graph unavailable"]'));
    assert(host.querySelector('[aria-label="Selected run journal"]'));
    assert(!host.querySelector('[aria-label="Node invoke"]'));
    const calls: string[] = [];
    await act(async () => root.render(<BlueprintProjectList workspaceId="workspace" api={{ list: async (workspace) => { calls.push(workspace); return [{ path: ".domios/blueprints/viewer-fixture.yaml", name: source.name, intent: source.intent ?? null, error: null }, { path: ".domios/blueprints/bad.yaml", name: null, intent: null, error: "mismatch" }]; }, show: async (_, path) => { calls.push(path); return { path, document: source }; } }} />));
    assert.equal(calls.length, 0);
    await act(async () => button("List repository Blueprints").click());
    assert.deepEqual(calls, ["workspace"]);
    assert(!host.querySelector('[aria-label="View .domios/blueprints/bad.yaml"]'));
    await act(async () => button("View .domios/blueprints/viewer-fixture.yaml").click());
    assert(host.querySelector('[aria-label="Repository source .domios/blueprints/viewer-fixture.yaml"]'));
    assert(![...host.querySelectorAll("button")].some((button) => /^(apply|run|approve)$/i.test(button.getAttribute("aria-label") ?? button.textContent ?? "")));
    const large = { ...viewerPipeline(), runs: [], nested_run: null,
      call_graph: { ...source, steps: Array.from({ length: 24 }, (_, index) => ({ ...source.steps[2], id: `call-${index}`, routes: { done: "@next", failed: "@stop" } })) },
      expanded_calls: Array.from({ length: 24 }, (_, index) => ({ ...call, node: `call-${index}` })),
    };
    let maxRender = 0, inputLatency = -1, scrollLatency = -1, sentAt = 0;
    await act(async () => root.render(<Profiler id="large-viewer" onRender={(_, __, duration) => { maxRender = Math.max(maxRender, duration); }}><input aria-label="Responsiveness probe" onInput={() => { inputLatency = performance.now() - sentAt; }} /><div aria-label="Scroll probe" onScroll={() => { scrollLatency = performance.now() - sentAt; }}><BlueprintPipelineCanvas pipeline={large} /></div></Profiler>));
    await act(async () => { sentAt = performance.now(); host.querySelector<HTMLInputElement>('[aria-label="Responsiveness probe"]')!.dispatchEvent(new Event("input", { bubbles: true })); sentAt = performance.now(); host.querySelector('[aria-label="Scroll probe"]')!.dispatchEvent(new Event("scroll")); });
    assert(inputLatency >= 0 && scrollLatency >= 0);
    await waitForNode("Node call-23");
    await act(async () => button("Node call-0").click());
    await act(async () => button("Expand callee").click());
    await waitForNode("Node return-count in call-0");
    console.log(`BlueprintPipelineCanvas: accessible interactions passed; 24 root calls + expanded callee; headless maxReactRender=${maxRender.toFixed(3)}ms inputDispatch=${inputLatency.toFixed(3)}ms scrollDispatch=${scrollLatency.toFixed(3)}ms`);
  } finally { await act(async () => root.unmount()); host.remove(); await window.happyDOM.close(); Object.defineProperty(globalThis, "MessageChannel", { configurable: true, value: nativeMessageChannel }); }
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
