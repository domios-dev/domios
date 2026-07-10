
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
