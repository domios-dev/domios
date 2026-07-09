import type { LaidOutNode } from "./layout";
import type { PresenceAgent } from "./presence/identity";
import type { World } from "./worlds";

export const MAX_ACTIVE_PRESENCE_OVERLAYS = 48;

export interface PresenceSource {
  /** Lead executor first, then peers — identity-keyed (same color everywhere). */
  agents: PresenceAgent[];
  /** The task is actively executing → the lead avatar gets a live identity ring. */
  live: boolean;
  /** Lead label for the W2+ name flag. */
  label: string;
  /** What it's doing right now (current focus / latest stride) — quiet W2 line. */
  action?: string | null;
  /** In-flight with no recent activity — dimmed pulse / warning tint. */
  stale?: boolean;
}

export interface PresenceOverlay {
  taskId: string;
  agents: PresenceAgent[];
  live: boolean;
  label: string;
  /** What it's doing right now — surfaced as a quiet truncated line at W2. */
  action: string | null;
  /** In-flight with no recent activity — dimmed pulse / warning tint. */
  stale?: boolean;
  world: World;
  /** World-space anchor — the card's top-right corner; parent layer carries the camera. */
  anchor: { x: number; y: number };
  /** Apparent on-screen card width — drives budget priority + LOD band fallback. */
  apparentWidth: number;
}

export interface BuildPresenceOverlaysInput {
  /** Only active tasks (the swarm) — keyed by task id. Iterated directly: O(active). */
  presenceByTask: ReadonlyMap<string, PresenceSource>;
  nodeById: ReadonlyMap<string, LaidOutNode>;
  /** Viewport-culled visible node ids — off-screen tasks emit nothing. */
  visibleIds: ReadonlySet<string>;
  worldByNode: ReadonlyMap<string, World>;
  cameraScale: number;
  maxActive?: number;
}

export function buildPresenceOverlays({
  presenceByTask,
  nodeById,
  visibleIds,
  worldByNode,
  cameraScale,
  maxActive = MAX_ACTIVE_PRESENCE_OVERLAYS,
}: BuildPresenceOverlaysInput): PresenceOverlay[] {
  const overlays: PresenceOverlay[] = [];
  for (const [taskId, src] of presenceByTask) {
    if (!src.agents.length || !src.agents[0] || !visibleIds.has(taskId)) continue;
    const node = nodeById.get(taskId);
    if (!node) continue;
    overlays.push({
      taskId,
      agents: src.agents,
      live: src.live,
      label: src.label,
      action: src.action ?? null,
      stale: src.stale === true,
      world: worldByNode.get(taskId) ?? "W0",
      anchor: { x: node.box.x + node.box.width, y: node.box.y },
      apparentWidth: node.box.width * cameraScale,
    });
  }
  // a working agent never loses its pin to an idle one. Deterministic id tiebreak.
  overlays.sort(
    (a, b) =>
      b.apparentWidth - a.apparentWidth ||
      Number(b.live) - Number(a.live) ||
      a.taskId.localeCompare(b.taskId),
  );
  return overlays.length > maxActive ? overlays.slice(0, maxActive) : overlays;
}
