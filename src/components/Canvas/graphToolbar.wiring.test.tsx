import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import { BlueprintCanvas } from "../Automation/BlueprintCanvas";
import { MemorySnapshotCanvas } from "../MemoryPanel/MemorySnapshotCanvas";
import { MeshCanvas } from "../MeshPanel/MeshCanvas";
import { layoutBlueprint } from "../Automation/blueprintLayout";
import { blueprintModel } from "../Automation/blueprintModel";
import { typedGraph } from "../Automation/blueprintModel.testdata";
import { createAutomationCamera } from "../Automation/useAutomationViewport";

const fakes = vi.hoisted(() => ({ fit: vi.fn(), zoom: vi.fn(), fly: vi.fn(), animate: vi.fn(), zoomIn: vi.fn(), zoomOut: vi.fn() }));
vi.mock("./useCanvasViewport", () => ({
  formatLayerTransform: () => "",
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, fitToView: fakes.fit, animateToView: fakes.fly, zoomBy: fakes.zoom }),
}));
vi.mock("../Automation/useAutomationViewport", async (original) => ({
  ...await original<typeof import("../Automation/useAutomationViewport")>(),
  useAutomationViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, touchHandlers: {}, zoom: fakes.zoom, fit: fakes.fit }),
}));
vi.mock("sigma", () => ({ default: class {
  on() {} off() {} kill() {} refresh() {}
  getCamera() { return { ratio: 1, on() {}, off() {}, animate: fakes.animate, animatedZoom: fakes.zoomIn, animatedUnzoom: fakes.zoomOut }; }
  getDimensions() { return { width: 600, height: 400 }; }
  viewportToGraph() { return { x: 0, y: 0 }; }
} }));
vi.mock("@sigma/edge-curve", () => ({ default: class {} }));
vi.mock("../MemoryPanel/memoryApi", () => ({ fetchLabels: async () => [], fetchGraphEdges: async () => [], setPinnedPosition: async () => {} }));
vi.mock("../MeshPanel/PeerTerminal", () => ({ PeerTerminal: () => null }));
vi.mock("../MeshPanel/ConnectionLine", () => ({ ConnectionLine: () => null }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
beforeEach(() => { vi.clearAllMocks(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });
async function mount(view: ReactNode) { await act(async () => root.render(view)); vi.clearAllMocks(); expect(host.querySelectorAll('[role="toolbar"][data-testid="canvas-toolbar"]')).toHaveLength(1); }
async function click(name: string) { const button = host.querySelector<HTMLButtonElement>(`[role="toolbar"] button[aria-label="${name}"]`); expect(button).not.toBeNull(); await act(async () => button!.click()); }

it("essence map wires the shared toolbar and selector once", async () => {
  const refresh = vi.fn();
  await mount(<EssenceMapCanvasInner workspace={{ id: "ws", name: "Fixture", tabs: [], active_tab_id: null, cwd: "/repo" }} mapFiles={[{ project_path: "/repo", map_path: "/repo/essences/map.jsonl", status: "ready", content: JSON.stringify({ kind: "node", id: "vector:fixture", type: "vector", label: "Fixture", vector: null, source: { path: "essences/vision.md", anchor: "vector:fixture", sha256: "a".repeat(64) } }) }]} onRetry={refresh} />);
  await click("Fit all"); expect(fakes.fit).toHaveBeenCalledTimes(1);
  await click("Zoom out"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(-0.25); fakes.zoom.mockClear();
  await click("Zoom in"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(0.25);
  await click("Refresh"); expect(refresh).toHaveBeenCalledTimes(1);
  const selector = host.querySelector<HTMLSelectElement>('[role="toolbar"] select')!;
  await act(async () => { selector.value = "/repo"; selector.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(fakes.fly).toHaveBeenCalledTimes(1);
});
it("Blueprint wires the shared toolbar once", async () => {
  await mount(<BlueprintCanvas layout={layoutBlueprint(blueprintModel(typedGraph()))} selected={null} camera={createAutomationCamera()} onSelect={() => {}} replay={null} />);
  await click("Fit all"); expect(fakes.fit).toHaveBeenCalledTimes(1);
  await click("Zoom out"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(-0.2); fakes.zoom.mockClear();
  await click("Zoom in"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(0.2);
  expect(host.querySelector('[role="toolbar"] button[aria-label="Refresh"]')).toBeNull();
});
it("memory wires the shared toolbar to its camera and refresh once", async () => {
  const refresh = vi.fn();
  await mount(<MemorySnapshotCanvas api={{ enabled: true, cors: false, host: "localhost", port: 7686, token: "fixture" }} index={{ byId: new Map([["fixture", { id: "fixture", kind: "note", x: 0, y: 0, importance: 0, has_vector: true, pinned: false, cluster_id: 0 }]]), clusters: [], clusterEdges: [], layoutVersion: 1, seq: 1, computedAt: "" }} pulses={new Map()} registerFit={() => {}} registerRecenter={() => {}} onRefresh={refresh} />);
  await click("Fit all"); expect(fakes.animate).toHaveBeenCalledTimes(1);
  await click("Zoom out"); expect(fakes.zoomOut).toHaveBeenCalledTimes(1);
  await click("Zoom in"); expect(fakes.zoomIn).toHaveBeenCalledTimes(1);
  await click("Refresh"); expect(refresh).toHaveBeenCalledTimes(1);
});
it("mesh wires the shared toolbar once", async () => {
  await mount(<MeshCanvas thisMachine={{ name: "Fixture", ip: "127.0.0.1" }} peers={[]} services={[]} recentPushes={[]} onPeerClick={() => {}} width={600} height={400} />);
  await click("Fit all"); expect(fakes.fit).toHaveBeenCalledTimes(1);
  await click("Zoom out"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(-0.06); fakes.zoom.mockClear();
  await click("Zoom in"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(0.06);
  expect(host.querySelector('[role="toolbar"] button[aria-label="Refresh"]')).toBeNull();
});
