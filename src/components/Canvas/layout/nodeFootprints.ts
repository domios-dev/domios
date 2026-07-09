import type { CanvasLayoutSettings } from "../settings";


export interface CanvasNodeFootprint {
  width: number;
  height: number;
}

export type NodeFootprintFn = (model: unknown, s: CanvasLayoutSettings) => CanvasNodeFootprint;

const footprints: Record<string, NodeFootprintFn> = {};

export function registerNodeFootprint(kind: string, fn: NodeFootprintFn): void {
  footprints[kind] = fn;
}

export function footprintFor(kind: string, model: unknown, s: CanvasLayoutSettings): CanvasNodeFootprint {
  const fn = footprints[kind];
  return fn ? fn(model, s) : { width: s.taskWidth, height: s.taskMinHeight };
}

// Test-only: clear plugin footprints between cases.
export function __resetNodeFootprintsForTest(): void {
  for (const k of Object.keys(footprints)) delete footprints[k];
}
