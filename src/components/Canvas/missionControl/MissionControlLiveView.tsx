import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Tab } from "../../../lib/types";
import { flattenSections } from "../sectionRows";
import {
  useCanvasViewport,
  paintLayer,
  CANVAS_MIN_SCALE,
  CANVAS_MAX_SCALE,
  type ViewportTransform,
} from "../useCanvasViewport";
import { FlyToActiveControl } from "../FlyToActiveControl";
import { selectActiveWork } from "../planView/activeWork";
import type { Plan } from "../types";
import { buildFlowRiver, type FlowRiverModel } from "./flowModel";
import { RIVER_WIDTH } from "./flowLayout";
import { buildMissionControl } from "./missionControl";
import { bumpMissionControlChurn } from "./missionControlChurn";
import { modelToSnapshot } from "./modelAdapter";
import { missionControlInputRevision } from "./missionControlInputRevision";
import { resolveMissionControlLod, type MissionControlLodLevel } from "./missionControlLod";
import { PlanRiver } from "./PlanRiver";
import { useReducedMotion } from "./useReducedMotion";
import { MC_PAN_IGNORE } from "./missionControlViewport";

export interface MissionControlLiveViewProps {
  plans: Plan[];
  activePlanId: string;
  tabs?: Record<string, Tab>;
  onTaskClick?: (taskId: string) => void;
  onOpenPlan?: (planId: string) => void;
  onAgentClick?: (tabId: string) => void;
}

interface RiverEntry {
  planId: string;
  model: FlowRiverModel;
  compact: boolean;
}

export function MissionControlLiveView({
  plans,
  activePlanId,
  tabs = {},
  onTaskClick,
  onOpenPlan,
  onAgentClick,
}: MissionControlLiveViewProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const scaleLabelRef = useRef<HTMLSpanElement | null>(null);
  const lodRef = useRef<MissionControlLodLevel>("close");
  const plansRef = useRef(plans);
  const tabsRef = useRef(tabs);
  plansRef.current = plans;
  tabsRef.current = tabs;
  const [size, setSize] = useState({ width: 800, height: 600 });
  const [lod, setLod] = useState<MissionControlLodLevel>("close");
  const reducedMotion = useReducedMotion();

  const inputRevision = useMemo(
    () => plans.map((p) => missionControlInputRevision(p, tabs)).join("|"),
    [plans, tabs],
  );

  const orderedPlans = useMemo(() => {
    const currentPlans = plansRef.current;
    const active = currentPlans.find((p) => p.id === activePlanId);
    const rest = currentPlans.filter((p) => p.id !== activePlanId);
    return active ? [active, ...rest] : currentPlans;
  }, [inputRevision, activePlanId]);

  const rivers = useMemo<RiverEntry[]>(() => {
    return orderedPlans.map((plan, i) => {
      const sectionTitle = new Map<string, string>();
      for (const s of flattenSections(plan.sections)) {
        const t = s.title?.trim();
        if (t) sectionTitle.set(s.id, t);
      }
      bumpMissionControlChurn("builds");
      const model = buildMissionControl(plan, tabsRef.current);
      bumpMissionControlChurn("snapshots");
      const snapshot = modelToSnapshot({
        plan,
        model,
        tabs: tabsRef.current,
        sectionTitleOf: (id) => sectionTitle.get(id) ?? null,
      });
      return {
        planId: plan.id,
        model: buildFlowRiver(plan, model, snapshot, tabsRef.current),
        compact: i > 0,
      };
    });
  }, [inputRevision, orderedPlans]);

  const activeWork = useMemo(() => {
    bumpMissionControlChurn("activeWork");
    return selectActiveWork(plansRef.current, tabsRef.current);
  }, [inputRevision]);

  const taskPlanId = useMemo(() => {
    const m = new Map<string, string>();
    for (const plan of plansRef.current) for (const t of plan.tasks) m.set(t.id, plan.id);
    return m;
  }, [inputRevision]);

  const syncLod = useCallback((scale: number) => {
    const next = resolveMissionControlLod(scale);
    if (next === lodRef.current) return;
    lodRef.current = next;
    setLod(next);
  }, []);

  const onPaint = useCallback(
    (t: ViewportTransform) => {
      syncLod(t.scale);
      const label = scaleLabelRef.current;
      if (label) label.textContent = `${Math.round(t.scale * 100)}%`;
    },
    [syncLod],
  );

  const { transform, handlers, fitToView, zoomBy, setTransform, isDragging, containerRef, layerRef, transformRef } =
    useCanvasViewport({
      width: size.width,
      height: size.height,
      minScale: CANVAS_MIN_SCALE,
      maxScale: CANVAS_MAX_SCALE,
      panButton: "any",
      ignoreSelector: MC_PAN_IGNORE,
      onPaint,
    });

  useLayoutEffect(() => {
    paintLayer(layerRef.current, transformRef.current);
    syncLod(transformRef.current.scale);
  }, [layerRef, transformRef, syncLod]);

  useLayoutEffect(() => {
    const el = shellRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (!rect) return;
      setSize({ width: rect.width, height: rect.height });
    });
    ro.observe(el);
    setSize({ width: el.clientWidth, height: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  const fittedRef = useRef(false);
  useEffect(() => {
    if (fittedRef.current || size.width <= 0) return;
    fittedRef.current = true;
    setTransform({ scale: 1, x: 0, y: 0 });
  }, [size.width, setTransform]);

  const stackHeight = 260 + Math.max(0, rivers.length - 1) * 180;
  const fitStage = useCallback(
    () => fitToView({ x: 0, y: 0, width: RIVER_WIDTH + 60, height: stackHeight }),
    [fitToView, stackHeight],
  );

  const flyToTask = useCallback(
    (taskId: string) => {
      const planId = taskPlanId.get(taskId);
      if (!planId) return;
      const el = shellRef.current?.querySelector(`[data-testid="plan-river-${planId}"]`);
      el?.scrollIntoView({ block: "center", behavior: reducedMotion ? "auto" : "smooth" });
    },
    [taskPlanId, reducedMotion],
  );

  return (
    <div
      ref={shellRef}
      className={`mc-root mc-live-shell mc-flow-shell${reducedMotion ? " mc-reduced-motion" : ""}`}
      data-testid="mission-control-live"
    >
      <header className="mc-live-toolbar">
        <div className="mc-live-zoom">
          <button type="button" className="mc-pill" onClick={() => zoomBy(-0.15)} aria-label="Zoom out">−</button>
          <span ref={scaleLabelRef} className="mc-live-scale" data-testid="mission-control-scale">
            {Math.round(transform.scale * 100)}%
          </span>
          <button type="button" className="mc-pill" onClick={() => zoomBy(0.15)} aria-label="Zoom in">+</button>
          <button type="button" className="mc-pill" onClick={fitStage}>Fit</button>
        </div>
      </header>
      <div
        ref={containerRef}
        className="mc-live-viewport"
        onMouseDown={handlers.onMouseDown}
        style={{ cursor: isDragging ? "grabbing" : "grab" }}
      >
        <div ref={layerRef} className="mc-stage-wrap mc-live-stage-wrap">
          <div className="mc-flow-stack" style={{ width: RIVER_WIDTH }}>
            {rivers.map((r) => (
              <PlanRiver
                key={r.planId}
                model={r.model}
                compact={r.compact}
                lod={lod}
                reducedMotion={reducedMotion}
                onTaskClick={onTaskClick}
                onAgentClick={onAgentClick}
                onOpenPlan={onOpenPlan}
              />
            ))}
          </div>
        </div>
      </div>
      <FlyToActiveControl entries={activeWork.entries} onFly={flyToTask} />
    </div>
  );
}
