import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Pane } from "../../../lib/types";
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
import { buildMissionControl } from "./missionControl";
import { missionControlHealth, modelToSnapshot } from "./modelAdapter";
import { missionControlInputRevision } from "./missionControlInputRevision";
import { resolveMissionControlLod, type MissionControlLodLevel } from "./missionControlLod";
import { MissionControlStage } from "./MissionControlStage";
import { useReducedMotion } from "./useReducedMotion";
import type { MissionControlSnapshot } from "./types";

import { MC_PAN_IGNORE } from "./missionControlViewport";

export interface MissionControlLiveViewProps {
  plan: Plan;
  panes?: Record<string, Pane>;
  onTaskClick?: (taskId: string) => void;
  onOpenPlan?: () => void;
  onAgentClick?: (paneId: string) => void;
}

export function MissionControlLiveView({ plan, panes = {}, onTaskClick, onOpenPlan, onAgentClick }: MissionControlLiveViewProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const scaleLabelRef = useRef<HTMLSpanElement | null>(null);
  const lodRef = useRef<MissionControlLodLevel>("close");
  const [size, setSize] = useState({ width: 800, height: 600 });
  const [lod, setLod] = useState<MissionControlLodLevel>("close");
  const reducedMotion = useReducedMotion();

  const inputRevision = useMemo(
    () => missionControlInputRevision(plan, panes),
    [plan, panes],
  );

  const sectionTitleById = useMemo(() => {
    const m = new Map<string, string>();
    for (const s of flattenSections(plan.sections)) {
      const t = s.title?.trim();
      if (t) m.set(s.id, t);
    }
    return m;
  }, [plan.id, inputRevision]);

  const model = useMemo(() => buildMissionControl(plan, panes), [inputRevision, plan, panes]);
  const snapshot: MissionControlSnapshot = useMemo(
    () =>
      modelToSnapshot({
        plan,
        model,
        panes,
        sectionTitleOf: (id) => sectionTitleById.get(id) ?? null,
      }),
    [inputRevision, plan, model, panes, sectionTitleById],
  );
  const health = useMemo(() => missionControlHealth(snapshot), [snapshot]);

  const activeWork = useMemo(() => selectActiveWork([plan], panes), [inputRevision, plan, panes]);

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

  const {
    transform,
    handlers,
    fitToView,
    zoomBy,
    setTransform,
    isDragging,
    containerRef,
    layerRef,
    transformRef,
  } = useCanvasViewport({
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

  const showEvents = !reducedMotion && lod === "close";

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
          <span ref={scaleLabelRef} className="mc-live-scale" data-testid="mission-control-scale">
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
        style={{ cursor: isDragging ? "grabbing" : "grab" }}
      >
        <div ref={layerRef} className="mc-stage-wrap mc-live-stage-wrap">
          <MissionControlStage
            snapshot={snapshot}
            lod={lod}
            dispatchGap={health.dispatchGap}
            qaBottleneck={health.qaBottleneck}
            showEvents={showEvents}
            onTaskClick={onTaskClick}
            onAgentClick={onAgentClick}
          />
        </div>
      </div>
      <FlyToActiveControl entries={activeWork.entries} onFly={focusTaskBay} />
    </div>
  );
}
