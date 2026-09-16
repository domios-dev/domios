import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import type { Workspace } from "../../lib/types";
import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import type { EssenceMapFile } from "./essenceMapModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const workspace: Workspace = {
  id: "map-gesture-workspace",
  name: "Gesture workspace",
  tabs: [],
  active_tab_id: null,
  cwd: "/repo/map",
};

const mapFile: EssenceMapFile = {
  project_path: "/repo/map",
  map_path: "/repo/map/essences/map.jsonl",
  status: "ready",
  content: JSON.stringify({
    kind: "node",
    id: "vector:gesture",
    type: "vector",
    label: "Gesture vector",
    vector: null,
    source: { path: "essences/CAPABILITIES.md", anchor: "vector:gesture", sha256: "a".repeat(64) },
  }),
};

const mounted: Array<{ host: HTMLDivElement; root: Root }> = [];

afterEach(async () => {
  await act(async () => {
    for (const { root } of mounted) root.unmount();
  });
  for (const { host } of mounted) host.remove();
  mounted.length = 0;
});

async function renderMap() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ host, root });
  await act(async () => root.render(<EssenceMapCanvasInner workspace={workspace} mapFiles={[mapFile]} />));
  return host;
}

function scaleOf(viewport: HTMLElement): number {
  const match = viewport.querySelector<HTMLElement>('[data-testid="essence-map-project-world"]')?.style.transform.match(/scale\(([^)]+)\)/);
  if (!match) throw new Error("map world transform was not painted");
  return Number(match[1]);
}

function wheel(viewport: HTMLElement, deltaY: number, modifiers: { ctrlKey?: boolean; metaKey?: boolean } = {}): WheelEvent {
  const event = new WheelEvent("wheel", {
    bubbles: true,
    cancelable: true,
    clientX: 120,
    clientY: 80,
    deltaY,
    ...modifiers,
  });
  viewport.dispatchEvent(event);
  return event;
}

describe("EssenceMapCanvas gesture ownership", () => {
  it("map_gesture_zoom binds wheel ownership to the receiving viewport", async () => {
    const host = await renderMap();
    const viewport = host.querySelector<HTMLElement>('[data-testid="essence-map-projects"]');
    expect(viewport).not.toBeNull();
    const receivingViewport = viewport!;
    const initial = scaleOf(receivingViewport);

    let plainIn!: WheelEvent;
    await act(async () => { plainIn = wheel(receivingViewport, -120); });
    const plainZoomIn = scaleOf(receivingViewport);
    expect(plainIn.defaultPrevented).toBe(true);
    expect(plainZoomIn).toBeGreaterThan(initial);

    let pinchOut!: WheelEvent;
    await act(async () => { pinchOut = wheel(receivingViewport, 120, { ctrlKey: true }); });
    const pinchZoomOut = scaleOf(receivingViewport);
    expect(pinchOut.defaultPrevented).toBe(true);
    expect(pinchZoomOut).toBeLessThan(plainZoomIn);
  });

  it("map_gesture_zoom keeps ctrl-wheel pinch independent from plain wheel", async () => {
    const host = await renderMap();
    const viewport = host.querySelector<HTMLElement>('[data-testid="essence-map-projects"]')!;
    const initial = scaleOf(viewport);

    await act(async () => { wheel(viewport, -120, { ctrlKey: true }); });
    const pinchZoomIn = scaleOf(viewport);
    await act(async () => { wheel(viewport, 120); });
    const plainZoomOut = scaleOf(viewport);

    expect(pinchZoomIn).toBeGreaterThan(initial);
    expect(plainZoomOut).toBeLessThan(pinchZoomIn);
  });
});
