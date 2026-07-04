import { useCallback, useState } from "react";

export type NavKind = "plan" | "section" | "task" | "spec" | "agent" | "node";

export interface NavEntry {
  /** Identity for dedup + crumb keys (a node id, or section id). */
  id: string;
  kind: NavKind;
  label: string;
  /** Node id set to re-frame when flying back to this hop. */
  flyIds: string[];
}

export interface CanvasNavHistory {
  /** Hops AWAY from home (home/plan is prepended by the breadcrumb, not stored here). */
  path: NavEntry[];
  /** Record a hop (no-op if it repeats the current top — re-flying the same node). */
  push: (entry: NavEntry) => void;
  /** Drop the last hop; returns the resulting path so the caller can fly to its top. */
  back: () => void;
  /** Truncate to a crumb index (0-based into `path`); a negative index clears to home. */
  truncate: (index: number) => void;
  /** Clear all hops — back at the plan. */
  clear: () => void;
}

// Pure stack transforms (unit-tested without React).
export function pushEntry(path: NavEntry[], entry: NavEntry): NavEntry[] {
  return path[path.length - 1]?.id === entry.id ? path : [...path, entry];
}
export function truncatePath(path: NavEntry[], index: number): NavEntry[] {
  return index < 0 ? [] : path.slice(0, index + 1);
}

export function useCanvasNavHistory(): CanvasNavHistory {
  const [path, setPath] = useState<NavEntry[]>([]);
  const push = useCallback((entry: NavEntry) => setPath((p) => pushEntry(p, entry)), []);
  const back = useCallback(() => setPath((p) => p.slice(0, -1)), []);
  const truncate = useCallback((index: number) => setPath((p) => truncatePath(p, index)), []);
  const clear = useCallback(() => setPath([]), []);
  return { path, push, back, truncate, clear };
}
