import { useEffect, useMemo } from "react";
import type { ViewportTransform } from "../useCanvasViewport";
import type { World } from "../worlds";
import { MAX_ACTIVE_PRESENCE_OVERLAYS, buildPresenceOverlays, type PresenceSource } from "../presenceOverlays";
import { buildSwarmRoster } from "../swarmRoster";
import { MAX_ACTIVE_RICH_OVERLAYS, buildOverlayNodes } from "../richOverlays";
import { buildCanvasScene, publishCanvasSceneReadback } from "../scene";
import type { CanvasLookups } from "./models";

interface OverlaysDeps {
  scene: ReturnType<typeof buildCanvasScene>;
  transform: ViewportTransform;
  focusedNodeId: string | null;
  mode: "workspace" | "focus";
  planId?: string;
  lookups: CanvasLookups;
  nodeIds: Set<string>;
  worldByNode: Map<string, World>;
}

// Camera-coupled overlay layers (rich content, presence pins, swarm roster) + the
// scene readback publish. Each memo keeps its original dep array so it recomputes
// only on the same triggers as before — swarmRoster stays transform-DECOUPLED.
export function useCanvasOverlays({ scene, transform, focusedNodeId, mode, planId, lookups, nodeIds, worldByNode }: OverlaysDeps) {
  const richOverlays = useMemo(() => buildOverlayNodes({ scene, transform, focusedId: focusedNodeId }), [scene, transform, focusedNodeId]);
  useEffect(
    () => publishCanvasSceneReadback(scene, transform, `${mode}:${planId ?? "workspace"}`, richOverlays, MAX_ACTIVE_RICH_OVERLAYS),
    [scene, transform, mode, planId, richOverlays],
  );

  // EMERGENCE: the swarm (only tasks with executing agents). Built off the existing
  // per-task presence — O(tasks) once per data change, NOT per pan/zoom.
  const presenceByTask = useMemo(() => {
    const m = new Map<string, PresenceSource>();
    for (const [id, model] of lookups.taskModelById) {
      const p = model.presence;
      if (!p || p.agents.length === 0) continue;
      m.set(id, { agents: p.agents, live: model.task.status === "in_progress", label: p.agents[0]?.label ?? p.label });
    }
    return m;
  }, [lookups.taskModelById]);
  // Floating presence pins: budget-bounded + viewport-culled → constant pin count on
  // a 20k board, recomputed only when camera/visibility/swarm change (no per-frame O(N)).
  const presenceOverlays = useMemo(
    () => buildPresenceOverlays({ presenceByTask, nodeById: lookups.nodeById, visibleIds: nodeIds, worldByNode, transform, maxActive: MAX_ACTIVE_PRESENCE_OVERLAYS }),
    [presenceByTask, lookups.nodeById, nodeIds, worldByNode, transform],
  );
  // EMERGENCE top-right cluster: the WHOLE swarm deduped to one circle per distinct
  // agent. Camera-DECOUPLED (no transform dep) so the screen-space faces stay put
  // while you fly; recomputed only when the swarm/task data changes.
  const swarmRoster = useMemo(
    () => buildSwarmRoster({ presenceByTask, taskById: lookups.taskById }),
    [presenceByTask, lookups.taskById],
  );

  return { richOverlays, presenceOverlays, swarmRoster };
}
