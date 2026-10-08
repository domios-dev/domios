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
  function Replay() {
    const frame = useBlueprintReplay(steps, 0);
    return <>
      <output aria-label="Replay state">{frame?.done ? "done" : "playing"}</output>
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
  } finally {
    await act(async () => root.unmount());
    host.remove();
    document.body.removeAttribute("data-motion");
    await window.happyDOM.close();
  }
  console.log("Blueprint replay removes animated elements and stops travel for reduced or paused motion");
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
