import { registerNodeFootprint, type NodeFootprintFn } from "./layout/nodeFootprints";
import { registerCanvasPainter, setCanvasNodeExpandable, type CanvasPainter } from "./canvasRender/painterRegistry";

// Section navigation payload — a click on a plan/spec section flies to its tasks.
export interface SectionNavArgs {
  planId: string;
  sectionId: string;
  variant: "plan" | "spec";
}

// Plugin-extensible canvas node: ONE capability descriptor adds a node kind with no
// core surgery — a CANVAS PAINTER (draws the node ON the canvas + returns hit regions,
// dispatched by drawScene) and an optional layout footprint (else the default element
// box). The fullscreen read surface is rendered by canvasRender/DetailContent. Built-in
// painters for plan/spec/task/handoff/plan-ghost live in canvasRender/builtinPainters.
export interface CanvasNodePlugin {
  kind: string;
  painter?: CanvasPainter;
  footprint?: NodeFootprintFn;
  capabilities?: { expandable?: boolean };
}

export function registerCanvasNode(plugin: CanvasNodePlugin): void {
  if (plugin.painter) registerCanvasPainter(plugin.kind, plugin.painter);
  if (plugin.footprint) registerNodeFootprint(plugin.kind, plugin.footprint);
  if (plugin.capabilities?.expandable != null) setCanvasNodeExpandable(plugin.kind, plugin.capabilities.expandable);
}
