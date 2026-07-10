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

const EMPTY_STALE_ACTIVE_TASK_IDS = new Set<string>();

export function useCanvasOverlays({ scene, transform, mode, planId, lookups, nodeIds, worldByNode, staleActiveTaskIds = EMPTY_STALE_ACTIVE_TASK_IDS }: OverlaysDeps) {
  useEffect(
    () => publishCanvasSceneReadback(scene, transform, `${mode}:${planId ?? "workspace"}`),
    [scene, transform, mode, planId],
  );

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
  // a 20k board, recomputed only when camera/visibility/swarm change (no per-frame O(N)).
  const presenceOverlays = useMemo(
    () => {
      bumpCanvasChurn("presenceOverlayBuilds");
      return buildPresenceOverlays({ presenceByTask, nodeById: lookups.nodeById, visibleIds: nodeIds, worldByNode, cameraScale: transform.scale, maxActive: MAX_ACTIVE_PRESENCE_OVERLAYS });
    },
    [presenceByTask, lookups.nodeById, nodeIds, worldByNode, transform.scale],
  );
  const swarmRoster = useMemo(
    () => {
      bumpCanvasChurn("swarmRosterBuilds");
      return buildSwarmRoster({ presenceByTask, taskById: lookups.taskById });
    },
    [presenceByTask, lookups.taskById],
  );

  return { presenceOverlays, swarmRoster };
}
