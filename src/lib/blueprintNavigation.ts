import type { BlueprintProjection } from "./blueprintApi";
import { navigationId, navigationObject, optionalNavigationId } from "./viewerNavigation";

export interface BlueprintNavigation {
  runId: string | null;
  trail: readonly string[];
  selected: string | null;
}

export function blueprintNavigation(value: unknown): BlueprintNavigation | undefined {
  const route = navigationObject(value);
  const runId = optionalNavigationId(route?.runId);
  const trail = route?.trail;
  if (runId === undefined || !Array.isArray(trail) || trail.length > 16 || !trail.every(navigationId)) return undefined;
  return { runId, trail, selected: optionalNavigationId(route?.selected) ?? null };
}

export function restoreBlueprintNavigation(route: BlueprintNavigation, pipeline: BlueprintProjection & { runs?: readonly { id: string; pipeline: BlueprintProjection }[] }): BlueprintNavigation {
  const run = route.runId === null ? undefined : pipeline.runs?.find((entry) => entry.id === route.runId);
  const runId = run ? route.runId : null;
  const source = run?.pipeline ?? pipeline;
  let document = source.call_graph;
  let calls = source.expanded_calls ?? [];
  const trail: string[] = [];
  if (route.runId !== null && runId === null) return { runId, trail, selected: null };
  for (const node of route.trail) {
    const call = calls.find((entry) => entry.node === node);
    if (!call) break;
    trail.push(node);
    document = call.graph;
    calls = call.children;
  }
  let selected: string | null = null;
  try {
    const path: unknown = route.selected ? JSON.parse(route.selected) : null;
    if (Array.isArray(path) && path.length === trail.length + 1 && trail.every((node, index) => path[index] === node)) {
      const id: unknown = path[path.length - 1];
      if (id === "$trigger" || id === "$result" || document?.steps.some((node) => node.id === id) || document?.other?.some((node) => node.id === id)) selected = route.selected;
    }
  } catch {}
  return { runId, trail, selected };
}
