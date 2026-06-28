import type { CanvasLayoutSettings } from "../settings";

// Plugin-extensible footprint seam: a custom canvas node kind can declare how big its
// box is so the layout sizes it WITHOUT core surgery. Lightweight on purpose (only a
// settings type) so the layout can consult it with no React/registry import cycle.

export interface CanvasNodeFootprint {
  width: number;
  height: number;
}

export type NodeFootprintFn = (model: unknown, s: CanvasLayoutSettings) => CanvasNodeFootprint;

const footprints: Record<string, NodeFootprintFn> = {};

export function registerNodeFootprint(kind: string, fn: NodeFootprintFn): void {
  footprints[kind] = fn;
}

// The box for a free-form / plugin element. Dispatch by capability ("does this kind
// declare a footprint?"), never a hardcoded kind list — the default is today's
// element box, so built-ins and unregistered kinds are unchanged.
export function footprintFor(kind: string, model: unknown, s: CanvasLayoutSettings): CanvasNodeFootprint {
  const fn = footprints[kind];
  return fn ? fn(model, s) : { width: s.taskWidth, height: s.taskMinHeight };
}

// Test-only: clear plugin footprints between cases.
export function __resetNodeFootprintsForTest(): void {
  for (const k of Object.keys(footprints)) delete footprints[k];
}
