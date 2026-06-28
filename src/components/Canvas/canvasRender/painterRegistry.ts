// The LIVE canvas render seam: a node kind is drawn ON the canvas by a CanvasPainter.
// A painter is a pure fn (ctx, {node, model, w, h, palette}) => hitRegions that paints
// the node to the 2D context and returns its hit regions — the exact shape drawCard
// already implements. drawScene dispatches through this registry; plan/spec/task are
// the built-in painters (builtinPainters.ts). A plugin registers its own painter to
// draw a custom node ON the canvas and be hit-tested — extensibility without DOM.

import type { LaidOutNode } from "../layout";
import type { HitRegion } from "./drawCard";
import type { CanvasPalette } from "./palette";

export interface CanvasPaintArgs {
  node: LaidOutNode;
  model: unknown;
  w: number;
  h: number;
  palette: CanvasPalette;
}

export interface CanvasPainter {
  /** Stable cache key for this model — re-rasterize only when it changes. null = skip. */
  signature: (model: unknown) => string | null;
  /** Paint the node at logical size w×h (ctx is dpr-scaled) and return hit regions. */
  paint: (ctx: CanvasRenderingContext2D, args: CanvasPaintArgs) => HitRegion[];
}

const painters = new Map<string, CanvasPainter>();
const expandableByKind = new Map<string, boolean>();

export function registerCanvasPainter(kind: string, painter: CanvasPainter): void {
  painters.set(kind, painter);
}

export function getCanvasPainter(kind: string): CanvasPainter | undefined {
  return painters.get(kind);
}

// Whether a node kind shows the Open/expand pill. Default true (built-ins expand); a
// plugin opts out with capabilities.expandable=false — drawCard reads this.
export function setCanvasNodeExpandable(kind: string, expandable: boolean): void {
  expandableByKind.set(kind, expandable);
}

export function isCanvasNodeExpandable(kind: string): boolean {
  return expandableByKind.get(kind) ?? true;
}

// Test-only: drop a painter registered by a spec.
export function __unregisterCanvasPainterForTest(kind: string): void {
  painters.delete(kind);
  expandableByKind.delete(kind);
}
