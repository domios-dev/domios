import type Graph from "graphology";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { EssenceMapCanvasInner } from "./EssenceMapCanvas";
import { BlueprintCanvas } from "../Automation/blueprint/Canvas";
import { MemorySnapshotCanvas } from "../MemoryPanel/MemorySnapshotCanvas";
import { MeshCanvas } from "../MeshPanel/MeshCanvas";
import { layoutBlueprint } from "../Automation/blueprint/layout";
import { buildBlueprintGraph } from "../Automation/blueprint/graphModel";
import { viewerPipeline } from "../Automation/blueprintViewer.testdata";
import { createAutomationCamera } from "../Automation/blueprint/viewport";

const fakes = vi.hoisted(() => ({ fit: vi.fn(), zoom: vi.fn(), fly: vi.fn(), animate: vi.fn(), zoomIn: vi.fn(), zoomOut: vi.fn(), pin: vi.fn(async () => {}), unpin: vi.fn(async () => {}), events: new Map<string, Set<(payload: unknown) => void>>() }));
vi.mock("@codeterm/canvas-camera", async (original) => ({
  ...await original<typeof import("@codeterm/canvas-camera")>(),
  formatLayerTransform: () => "",
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, transformRef: { current: { x: 0, y: 0, scale: 1 } }, setTransform: () => {}, fitToView: fakes.fit, animateToView: fakes.fly, zoomBy: fakes.zoom }),
}));
vi.mock("../Automation/blueprint/viewport", async (original) => ({
  ...await original<typeof import("../Automation/blueprint/viewport")>(),
  useAutomationViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, touchHandlers: {}, zoom: fakes.zoom, fit: fakes.fit, showOverview: fakes.fit }),
}));
vi.mock("sigma", () => ({ default: class {
  constructor(private graph: Graph) {}
  on(name: string, handler: (payload: unknown) => void) { const handlers = fakes.events.get(name) ?? new Set(); handlers.add(handler); fakes.events.set(name, handlers); }
  off() {} kill() {} refresh() {}
  getStagePadding() { return 0; }
  getNodeDisplayData(id: string) { return { ...this.graph.getNodeAttributes(id), x: 0.35, y: 0.4 }; }
  setCustomBBox() {} getCustomBBox() { return null; } getBBox() { return { x: [0, 1], y: [0, 1] }; }
  viewportToFramedGraph({ x, y }: { x: number; y: number }) { return { x: x / 400, y: y / 400 }; }
  graphToViewport() { return { x: 10, y: 10 }; }
  getCamera() { return { ratio: 1, on() {}, off() {}, animate: fakes.animate, zoomIn: fakes.zoomIn, zoomOut: fakes.zoomOut }; }
  getDimensions() { return { width: 600, height: 400 }; }
  viewportToGraph({ x, y }: { x: number; y: number }) { return { x, y }; }
} }));
vi.mock("sigma/rendering", () => ({ pathCurved: () => ({ name: "curved" }) }));
vi.mock("../MemoryPanel/memoryApi", () => ({ fetchLabels: async () => [], fetchGraphEdges: async () => [], setPinnedPosition: fakes.pin, clearPinnedPosition: fakes.unpin }));
vi.mock("../MeshPanel/PeerTerminal", () => ({ PeerTerminal: () => null }));
vi.mock("../MeshPanel/ConnectionLine", () => ({ ConnectionLine: () => null }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
beforeEach(() => { vi.clearAllMocks(); fakes.events.clear(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
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
  const selector = Array.from(host.querySelectorAll<HTMLButtonElement>("button[aria-label]"))
    .find((button) => button.getAttribute("aria-label")?.startsWith("Fly to project map:"));
  expect(selector).not.toBeNull();
  await act(async () => selector!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
  const item = document.body.querySelector<HTMLButtonElement>('[role="menuitem"], [role="menuitemradio"]')!;
  expect(item).not.toBeNull();
  await act(async () => item.click());
  expect(fakes.fly).toHaveBeenCalledTimes(1);
});
it("Blueprint wires the shared toolbar once", async () => {
  const graph = buildBlueprintGraph(viewerPipeline(), new Set());
  let built = graph.next();
  while (!built.done) built = graph.next();
  await mount(<BlueprintCanvas layout={layoutBlueprint(built.value)} label="Blueprint graph" selected={null} camera={createAutomationCamera()} onSelect={() => {}} lit={new Set()} />);
  await click("Fit all"); expect(fakes.fit).toHaveBeenCalledTimes(1);
  await click("Zoom out"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(-0.25); fakes.zoom.mockClear();
  await click("Zoom in"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(0.25);
  expect(host.querySelector('[role="toolbar"] button[aria-label="Refresh"]')).toBeNull();
});
it("memory wires the shared toolbar to its camera and refresh once", async () => {
  const refresh = vi.fn();
  await mount(<MemorySnapshotCanvas api={{mesh_tailnet_host: null, cors_allow_origins: [], preview_secret_paths: [],  enabled: true, cors: false, host: "localhost", port: 7686, token: "fixture" }} index={{ byId: new Map([["fixture", { id: "fixture", kind: "note", x: 0, y: 0, importance: 0, has_vector: true, pinned: false, cluster_id: 0 }]]), clusters: [], clusterEdges: [], layoutVersion: 1, seq: 1, computedAt: "" }} pulses={new Map()} registerFit={() => {}} registerRecenter={() => {}} onRefresh={refresh} />);
  await click("Fit all"); expect(fakes.animate).toHaveBeenCalledExactlyOnceWith({ x: 0.35, y: 0.4, ratio: 0.05, angle: 0 }, { duration: 300 });
  await click("Zoom out"); expect(fakes.zoomOut).toHaveBeenCalledTimes(1);
  await click("Zoom in"); expect(fakes.zoomIn).toHaveBeenCalledTimes(1);
  await click("Refresh"); expect(refresh).toHaveBeenCalledTimes(1);
});
it("mesh wires the shared toolbar once", async () => {
  const refresh = vi.fn();
  await mount(<MeshCanvas onRefresh={refresh} thisMachine={{ name: "Fixture", ip: "127.0.0.1" }} peers={[]} recentPushes={[]} onPeerClick={() => {}} width={600} height={400} />);
  await click("Fit all"); expect(fakes.fit).toHaveBeenCalledTimes(1);
  await click("Zoom out"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(-0.25); fakes.zoom.mockClear();
  await click("Zoom in"); expect(fakes.zoom).toHaveBeenCalledExactlyOnceWith(0.25);
  await click("Refresh"); expect(refresh).toHaveBeenCalledTimes(1);
});

it("memory pins only after dragging, and the selected drawer can unpin", async () => {
  const api = { mesh_tailnet_host: null, cors_allow_origins: [], preview_secret_paths: [], enabled: true, cors: false, host: "localhost", port: 7686, token: "fixture" };
  const item = { id: "fixture", kind: "note", x: 0, y: 0, importance: 0, has_vector: true, pinned: true, cluster_id: 0 };
  const node = { ...item, title: "Fixture", summary: null, area: null, tags: null, has_body: true, body: "" };
  await mount(<MemorySnapshotCanvas api={api} index={{ byId: new Map([[item.id, item]]), clusters: [], clusterEdges: [], layoutVersion: 1, seq: 1, computedAt: "" }} metadataNodes={new Map([[node.id, node]])} pulses={new Map()} registerFit={() => {}} registerRecenter={() => {}} selectedIdOverride={item.id} />);
  const event = (x: number, y: number) => ({ x, y, preventSigmaDefault() {}, original: { preventDefault() {}, stopPropagation() {} } });
  const emit = async (name: string, x: number, y: number) => { await act(async () => { for (const handler of fakes.events.get(name) ?? []) handler({ node: item.id, event: event(x, y) }); }); };
  await emit("downNode", 10, 10);
  await emit("upNode", 10, 10);
  expect(fakes.pin).not.toHaveBeenCalled();
  await emit("downNode", 10, 10);
  await emit("moveBody", 12, 12);
  await emit("upNode", 12, 12);
  expect(fakes.pin).not.toHaveBeenCalled();
  await emit("downNode", 10, 10);
  await emit("moveBody", 16, 10);
  await emit("upStage", 16, 10);
  await emit("upNode", 16, 10);
  expect(fakes.pin).toHaveBeenCalledExactlyOnceWith(api, item.id, 16, -10);
  const unpin = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Unpin position");
  expect(unpin).toBeDefined();
  await act(async () => unpin!.click());
  expect(fakes.unpin).toHaveBeenCalledExactlyOnceWith(api, item.id);
  expect(Array.from(host.querySelectorAll("button")).some(button => button.textContent === "Unpin position")).toBe(false);
});
