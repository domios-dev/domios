import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Pane } from "../../../lib/types";
import { flattenSections } from "../sectionRows";
import { useCanvasViewport, CANVAS_MIN_SCALE, CANVAS_MAX_SCALE } from "../useCanvasViewport";
import { FlyToActiveControl } from "../FlyToActiveControl";
import { selectActiveWork } from "../planView/activeWork";
import type { Plan } from "../types";
import { buildMissionControl } from "./missionControl";
import { missionControlHealth, modelToSnapshot } from "./modelAdapter";
import { resolveMissionControlLod } from "./missionControlLod";
import { useReducedMotion } from "./useReducedMotion";
import type { MissionControlSnapshot } from "./types";
import { VariantAssemblyLine } from "./variants/VariantAssemblyLine";
import "./missionControl.css";

export interface MissionControlLiveViewProps {
  plan: Plan;
  panes?: Record<string, Pane>;
  onTaskClick?: (taskId: string) => void;
  onOpenPlan?: () => void;
}

export function MissionControlLiveView({ plan, panes = {}, onTaskClick, onOpenPlan }: MissionControlLiveViewProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 800, height: 600 });
  const reducedMotion = useReducedMotion();

  const sectionTitleById = useMemo(() => {
    const m = new Map<string, string>();
    for (const s of flattenSections(plan.sections)) {
      const t = s.title?.trim();
      if (t) m.set(s.id, t);
    }
    return m;
  }, [plan.sections, plan.id]);

  const model = useMemo(() => buildMissionControl(plan, panes), [plan, panes, plan.updated_at, plan.tasks.length]);
  const snapshot: MissionControlSnapshot = useMemo(
    () =>
      modelToSnapshot({
        plan,
        model,
        panes,
        sectionTitleOf: (id) => sectionTitleById.get(id) ?? null,
      }),
    [plan, model, panes, sectionTitleById],
  );
  const health = useMemo(() => missionControlHealth(snapshot), [snapshot]);

  const activeWork = useMemo(() => selectActiveWork([plan], panes), [plan, panes]);

  const { transform, handlers, fitToView, zoomBy, setTransform, isDragging, containerRef } = useCanvasViewport({
    width: size.width,
    height: size.height,
    minScale: CANVAS_MIN_SCALE,
    maxScale: CANVAS_MAX_SCALE,
    wheelBehavior: "zoom",
    panButton: "middle",
  });

  const lod = useMemo(() => resolveMissionControlLod(transform.scale), [transform.scale]);

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

  const fitStage = useCallback(() => fitToView({ x: 0, y: 0, width: 1120, height: 700 }), [fitToView]);

  const focusTaskBay = useCallback(
    (taskId: string) => {
      const lane = snapshot.lanes.find((l) => {
        const tasks = l.activeTasks ?? (l.activeTask ? [l.activeTask] : []);
        return tasks.some((t) => t.id === taskId);
      });
      if (lane) {
        const el = shellRef.current?.querySelector(`[data-testid="lane-bay-${lane.id}"]`);
        el?.scrollIntoView({ block: "nearest", inline: "center", behavior: reducedMotion ? "auto" : "smooth" });
      }
    },
    [snapshot.lanes, reducedMotion],
  );

  return (
    <div
      ref={shellRef}
      className={`mc-root mc-live-shell${reducedMotion ? " mc-reduced-motion" : ""}`}
      data-testid="mission-control-live"
    >
      <header className="mc-live-toolbar">
        <button type="button" className="mc-pill" data-active="true" onClick={onOpenPlan}>
          {snapshot.planTitle}
        </button>
        <div className="mc-live-zoom">
          <button type="button" className="mc-pill" onClick={() => zoomBy(-0.15)} aria-label="Zoom out">
            −
          </button>
          <span className="mc-live-scale" data-testid="mission-control-scale">
            {Math.round(transform.scale * 100)}%
          </span>
          <button type="button" className="mc-pill" onClick={() => zoomBy(0.15)} aria-label="Zoom in">
            +
          </button>
          <button type="button" className="mc-pill" onClick={fitStage}>
            Fit
          </button>
        </div>
      </header>
      <div
        ref={containerRef}
        className="mc-live-viewport"
        onMouseDown={handlers.onMouseDown}
        style={{ cursor: isDragging ? "grabbing" : "default" }}
      >
        <div
          className="mc-stage-wrap mc-live-stage-wrap"
          style={{
            transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})`,
            transformOrigin: "0 0",
          }}
        >
          <div
            className="mc-stage mc-live-stage"
            data-lod={lod}
            data-dispatch-gap={health.dispatchGap ? "true" : undefined}
            data-qa-bottleneck={health.qaBottleneck ? "true" : undefined}
          >
            <VariantAssemblyLine
              snapshot={snapshot}
              onTaskClick={onTaskClick}
              dispatchGap={health.dispatchGap}
              qaBottleneck={health.qaBottleneck}
              showEvents={!reducedMotion && lod === "close"}
            />
          </div>
        </div>
      </div>
      <FlyToActiveControl entries={activeWork.entries} onFly={focusTaskBay} />
    </div>
  );
}
