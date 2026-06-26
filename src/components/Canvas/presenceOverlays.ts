import type { LaidOutNode } from "./layout";
import type { PresenceAgent } from "./presence/identity";
import { projectWorldBox } from "./scene";
import type { ViewportTransform } from "./useCanvasViewport";
import type { World } from "./worlds";

// EMERGENCE (north-star #2): the swarm floats ON the canvas — each active agent is
// a presence pin pinned to the corner of the task it executes, so you literally SEE
// the swarm distributed by the work. Mirrors the rich-overlay budget discipline:
// a HARD cap on simultaneously-rendered pins. Pins are light DOM (an avatar + flag),
// so the budget is roomy vs the 2 heavy markdown rich overlays — but still bounded,
// so a 20k-task board emits a constant pin count, never O(N) per frame.
export const MAX_ACTIVE_PRESENCE_OVERLAYS = 48;

export interface PresenceSource {
  /** Lead executor first, then peers — identity-keyed (same color everywhere). */
  agents: PresenceAgent[];
  /** The task is actively executing → the lead avatar gets a live identity ring. */
  live: boolean;
  /** Lead label for the W2+ name flag. */
  label: string;
}

export interface PresenceOverlay {
  taskId: string;
  agents: PresenceAgent[];
  live: boolean;
  label: string;
  world: World;
  /** Screen-space anchor — the card's top-right corner; the pin straddles it. */
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
  transform: ViewportTransform;
  maxActive?: number;
}

// Pure + camera-decoupled in structure: iterates ONLY the (small) presence map,
// culls to the already-computed visible set, then caps to the budget. No scan of
// the full node set, so cost is O(active swarm), not O(total tasks).
export function buildPresenceOverlays({
  presenceByTask,
  nodeById,
  visibleIds,
  worldByNode,
  transform,
  maxActive = MAX_ACTIVE_PRESENCE_OVERLAYS,
}: BuildPresenceOverlaysInput): PresenceOverlay[] {
  const overlays: PresenceOverlay[] = [];
  for (const [taskId, src] of presenceByTask) {
    if (!src.agents.length || !visibleIds.has(taskId)) continue;
    const node = nodeById.get(taskId);
    if (!node) continue;
    const box = projectWorldBox(node.box, transform);
    overlays.push({
      taskId,
      agents: src.agents,
      live: src.live,
      label: src.label,
      world: worldByNode.get(taskId) ?? "W0",
      anchor: { x: box.x + box.width, y: box.y },
      apparentWidth: box.width,
    });
  }
  // Budget: keep the most prominent (largest on-screen) pins first; live wins ties so
  // a working agent never loses its pin to an idle one. Deterministic id tiebreak.
  overlays.sort(
    (a, b) =>
      b.apparentWidth - a.apparentWidth ||
      Number(b.live) - Number(a.live) ||
      a.taskId.localeCompare(b.taskId),
  );
  return overlays.length > maxActive ? overlays.slice(0, maxActive) : overlays;
}
