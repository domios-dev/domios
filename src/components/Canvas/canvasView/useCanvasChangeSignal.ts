import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ViewportTransform } from "../useCanvasViewport";
import { boxesOverlap, viewportWorldRect, type WorkspaceLayout } from "../layout";
import type { Plan } from "../types";

interface ChangeSignalDeps {
  filteredPlans: Plan[];
  layout: WorkspaceLayout;
  transform: ViewportTransform;
  canvasSize: { width: number; height: number };
  size: { width: number; height: number };
  fitCameraToNodes: (ids: string[], animate?: boolean) => void;
}

export function useCanvasChangeSignal({ filteredPlans, layout, transform, canvasSize, size, fitCameraToNodes }: ChangeSignalDeps) {
  const prevUpdatedRef = useRef<Map<string, number>>(new Map());
  const [changedPlanIds, setChangedPlanIds] = useState<Set<string>>(() => new Set());
  useEffect(() => {
    const prev = prevUpdatedRef.current;
    const changed = new Set<string>();
    for (const p of filteredPlans) {
      const was = prev.get(p.id);
      if (was != null && p.updated_at > was) changed.add(p.id);
      prev.set(p.id, p.updated_at);
    }
    for (const id of [...prev.keys()]) if (!filteredPlans.some((p) => p.id === id)) prev.delete(id);
    if (!changed.size) return;
    setChangedPlanIds(changed);
    const h = setTimeout(() => setChangedPlanIds(new Set()), 6000);
    return () => clearTimeout(h);
  }, [filteredPlans]);

  const { pulsePlanIds, changeMarkers } = useMemo(() => {
    const pulse = new Set<string>();
    const markers: { planId: string; x: number; y: number }[] = [];
    if (!changedPlanIds.size) return { pulsePlanIds: pulse, changeMarkers: markers };
    const viewRect = viewportWorldRect(transform, canvasSize, 0);
    const ccx = size.width / 2;
    const ccy = size.height / 2;
    const margin = 18;
    const halfW = Math.max(20, size.width / 2 - margin);
    const halfH = Math.max(20, size.height / 2 - margin);
    for (const f of layout.frames) {
      if (!changedPlanIds.has(f.planId)) continue;
      if (boxesOverlap(f.box, viewRect)) {
        pulse.add(f.planId);
        continue;
      }
      const cx = f.box.x + f.box.width / 2;
      const cy = f.box.y + f.box.height / 2;
      const sx = cx * transform.scale + transform.x;
      const sy = cy * transform.scale + transform.y;
      const dx = sx - ccx;
      const dy = sy - ccy;
      if (dx === 0 && dy === 0) continue;
      const tt = Math.min(dx !== 0 ? halfW / Math.abs(dx) : Infinity, dy !== 0 ? halfH / Math.abs(dy) : Infinity);
      markers.push({ planId: f.planId, x: ccx + dx * tt, y: ccy + dy * tt });
    }
    return { pulsePlanIds: pulse, changeMarkers: markers };
  }, [changedPlanIds, layout, transform, canvasSize, size.width, size.height]);

  const flyToPlan = useCallback(
    (planId: string) => {
      fitCameraToNodes([planId]);
      setChangedPlanIds((prev) => {
        if (!prev.has(planId)) return prev;
        const next = new Set(prev);
        next.delete(planId);
        return next;
      });
    },
    [fitCameraToNodes],
  );

  return { pulsePlanIds, changeMarkers, flyToPlan };
}
