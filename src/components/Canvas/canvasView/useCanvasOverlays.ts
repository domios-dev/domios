import { useEffect, useMemo, useState } from "react";
import type { ViewportTransform } from "../useCanvasViewport";
import type { World } from "../worlds";
import { MAX_ACTIVE_PRESENCE_OVERLAYS, buildPresenceOverlays, type PresenceSource } from "../presenceOverlays";
import { presenceAnimateDemoEnabled, rotatePresenceForDemo } from "../presenceDemo";
import { buildSwarmRoster } from "../swarmRoster";
import { buildCanvasScene, publishCanvasSceneReadback } from "../scene";
import type { CanvasLookups } from "./models";
import { bumpCanvasChurn } from "../canvasChurn";

interface OverlaysDeps {
  scene: ReturnType<typeof buildCanvasScene>;
  transform: ViewportTransform;
  focusedNodeId: string | null;
  mode: "workspace" | "focus";
  planId?: string;
  lookups: CanvasLookups;
  nodeIds: Set<string>;
  worldByNode: Map<string, World>;
  staleActiveTaskIds?: ReadonlySet<string>;
}

// Camera-coupled overlay layers (rich content, presence pins, swarm roster) + the
// scene readback publish. Each memo keeps its original dep array so it recomputes
// only on the same triggers as before — swarmRoster stays transform-DECOUPLED.
export function useCanvasOverlays({ scene, transform, mode, planId, lookups, nodeIds, worldByNode, staleActiveTaskIds = new Set() }: OverlaysDeps) {
  useEffect(
    () => publishCanvasSceneReadback(scene, transform, `${mode}:${planId ?? "workspace"}`),
    [scene, transform, mode, planId],
  );

  // EMERGENCE: the swarm (only tasks with executing agents). Built off the existing
  // per-task presence — O(tasks) once per data change, NOT per pan/zoom.
  const demoOn = presenceAnimateDemoEnabled();
  const [demoTick, setDemoTick] = useState(0);
  useEffect(() => {
    if (!demoOn) return;
    // Slower than a glide (presenceGlide GLIDE_DURATION_MS) so each hop settles.
    const id = window.setInterval(() => setDemoTick((t) => t + 1), 1600);
    return () => window.clearInterval(id);
  }, [demoOn]);
  const presenceByTask = useMemo(() => {
    bumpCanvasChurn("presenceSourceBuilds");
    const m = new Map<string, PresenceSource>();
    for (const [id, model] of lookups.taskModelById) {
      const p = model.presence;
      if (!p || !p.agents.length || !p.agents[0]) continue;
      // What it's doing now: orchestrator focus or latest stride — prefixed so it
      // never reads like plan progress (e.g. subtasks 0/8) or an ambiguous fraction.
      const action = p.focus
        ? `focus · ${p.focus}`
        : p.latest
          ? `stride · ${p.latest.kindLabel}`
          : null;
      m.set(id, {
        agents: p.agents,
        live: model.task.status === "in_progress" && !staleActiveTaskIds.has(id),
        label: p.agents[0]?.label ?? p.label,
        action,
        stale: staleActiveTaskIds.has(id),
      });
    }
    return demoOn ? rotatePresenceForDemo(m, demoTick) : m;
  }, [lookups.taskModelById, demoOn, demoTick, staleActiveTaskIds]);
  // Floating presence pins: budget-bounded + viewport-culled → constant pin count on
  // a 20k board, recomputed only when camera/visibility/swarm change (no per-frame O(N)).
  const presenceOverlays = useMemo(
    () => {
      bumpCanvasChurn("presenceOverlayBuilds");
      return buildPresenceOverlays({ presenceByTask, nodeById: lookups.nodeById, visibleIds: nodeIds, worldByNode, cameraScale: transform.scale, maxActive: MAX_ACTIVE_PRESENCE_OVERLAYS });
    },
    [presenceByTask, lookups.nodeById, nodeIds, worldByNode, transform.scale],
  );
  // EMERGENCE top-right cluster: the WHOLE swarm deduped to one circle per distinct
  // agent. Camera-DECOUPLED (no transform dep) so the screen-space faces stay put
  // while you fly; recomputed only when the swarm/task data changes.
  const swarmRoster = useMemo(
    () => {
      bumpCanvasChurn("swarmRosterBuilds");
      return buildSwarmRoster({ presenceByTask, taskById: lookups.taskById });
    },
    [presenceByTask, lookups.taskById],
  );

  return { presenceOverlays, swarmRoster };
}
