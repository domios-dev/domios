import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { BlueprintCanvas } from "./BlueprintCanvas";
import { layoutBlueprint } from "./blueprintLayout";
import { blueprintModel } from "./blueprintModel";
import { typedGraph } from "./blueprintModel.testdata";
import type { ReplayStep } from "./blueprintRunModel";
import { createAutomationCamera } from "./useAutomationViewport";
import { useBlueprintReplay } from "./useBlueprintReplay";
import { BlueprintReplayStatus } from "./BlueprintReplayStatus";
import type { BlueprintReplayOutcome } from "./blueprintReplayOutcome";
import { replayFrame } from "./blueprintRunModel";

async function main() {
  const window = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "HTMLElement", "Element", "SVGElement", "Node", "navigator"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (window as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  let reduced = false;
  const queries: MediaQueryList[] = [];
  const matchMedia = window.matchMedia.bind(window);
  window.matchMedia = (media) => {
    const query = matchMedia(media);
    Object.defineProperty(query, "matches", { get: () => reduced });
    queries.push(query as unknown as MediaQueryList);
    return query;
  };
  let callback: FrameRequestCallback | null = null;
  let scheduled = 0;
  let cancelled = 0;
  Object.assign(globalThis, {
    requestAnimationFrame: (next: FrameRequestCallback) => { callback = next; return ++scheduled; },
    cancelAnimationFrame: () => { cancelled++; callback = null; },
  });
  const layout = layoutBlueprint(blueprintModel(typedGraph()));
  const wire = layout.wires[0].wire;
  const steps: ReplayStep[] = [
    { nodes: [wire.from.node], wires: [] },
    { nodes: [wire.to.node], wires: [wire.id] },
  ];
  const camera = createAutomationCamera();
  function Replay({ snapshot = steps, nonce = 0 }: { snapshot?: ReplayStep[]; nonce?: number }) {
    const frame = useBlueprintReplay(snapshot, nonce);
    return <>
      <output aria-label="Replay state">{frame?.done ? "done" : "playing"}</output>
      <span role="progressbar" aria-label="Replay progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={(frame?.overall ?? 0) * 100} />
      <BlueprintCanvas layout={layout} selected={null} camera={camera} onSelect={() => {}} replay={frame ? { frame, live: false } : null} />
    </>;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Replay />));
    assert.ok(callback, "motion-enabled replay starts its clock");
    await act(async () => { callback?.(0); });
    await act(async () => { callback?.(780); });
    assert.ok(host.querySelector('[data-replay-motion="bead"]'), "a travelling wire renders its droplet");
    assert.ok(host.querySelector('[data-replay-motion="absorb"]'), "an activated node renders its settle pulse");
    const progress = () => Number(host.querySelector('[role="progressbar"]')?.getAttribute("aria-valuenow"));
    assert.equal(progress(), 75);
    const duringTravel = { scheduled, cancelled, callback };
    await act(async () => root.render(<Replay snapshot={structuredClone(steps)} />));
    assert.equal(progress(), 75, "an equal snapshot preserves elapsed replay time");
    assert.deepEqual({ scheduled, cancelled, callback }, duringTravel, "refreshing the graph does not replace the running clock");
    assert.ok(host.querySelector('[data-replay-motion="bead"]'), "travel remains visible across a snapshot refresh");
    await act(async () => { callback?.(1040); });
    assert.equal(host.querySelector("output")?.textContent, "done", "the original clock reaches the end");
    assert.equal(progress(), 100);
    const afterFinish = { scheduled, cancelled };
    await act(async () => root.render(<Replay snapshot={structuredClone(steps)} />));
    assert.equal(host.querySelector("output")?.textContent, "done", "a later poll keeps the completed replay finished");
    assert.deepEqual({ scheduled, cancelled }, afterFinish, "a completed replay schedules no new clock on refresh");
    assert.equal(host.querySelector('[data-replay-motion="bead"]'), null);
    await act(async () => root.render(<Replay snapshot={structuredClone(steps)} nonce={1} />));
    assert.equal(progress(), 0, "an explicit replay click restarts the same path");
    assert.equal(host.querySelector("output")?.textContent, "playing");
    await act(async () => { callback?.(2000); });
    await act(async () => { callback?.(2780); });
    assert.equal(progress(), 75);
    assert.ok(host.querySelector('[data-replay-motion="bead"]'));
    const before = scheduled;
    await act(async () => {
      reduced = true;
      for (const query of queries) query.dispatchEvent(new window.Event("change") as unknown as Event);
    });
    assert.equal(host.querySelector("output")?.textContent, "done");
    assert.equal(host.querySelectorAll("[data-replay-motion], animate, animateMotion").length, 0);
    assert.equal(scheduled, before, "reduced motion schedules no further replay frames");
    assert.ok(cancelled > 0, "changing the motion preference cancels travel");
    assert.ok(host.querySelector('[role="region"][aria-label="Automation graph"]'));
    assert.ok(host.querySelectorAll("button[aria-label]").length > 2, "graph controls and nodes remain available");

    await act(async () => root.render(<div />));
    reduced = false;
    document.body.setAttribute("data-motion", "paused");
    await act(async () => root.render(<Replay />));
    assert.equal(host.querySelector("output")?.textContent, "done");
    assert.equal(host.querySelectorAll("[data-replay-motion], animate, animateMotion").length, 0);
    assert.equal(scheduled, before, "paused motion renders a static path without a replay clock");
    const outcome: BlueprintReplayOutcome = { nodeKey: wire.to.node, failed: true, noAction: false, reason: "recorded failure summary", label: "Stopped at supplied node: recorded failure summary" };
    await act(async () => root.render(<>
      <BlueprintReplayStatus done at={100} outcome={outcome} />
      <BlueprintCanvas layout={layout} selected={null} camera={camera} onSelect={() => {}} replay={{ frame: replayFrame(steps, 1040, false), live: false, outcome }} />
    </>));
    assert.equal(host.querySelector('[role="status"]')?.textContent, outcome.label, "the final header renders the supplied outcome");
    const stopped = [...host.querySelectorAll("button")].find((button) => button.getAttribute("aria-label")?.includes(outcome.label));
    assert.ok(stopped, "the stopped node exposes its outcome in its accessible name");
    assert.ok(stopped.textContent?.includes("Error"), "the stopped node renders its error badge");
    assert.equal(host.querySelectorAll("[data-replay-motion]").length, 0);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    document.body.removeAttribute("data-motion");
    await window.happyDOM.close();
  }
  console.log("Blueprint replay survives snapshot refreshes, finishes once per click, and honors static motion");
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
