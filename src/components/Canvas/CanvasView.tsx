import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import type { ApiConfig, Pane } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { CanvasComposer } from "./CanvasComposer";
import { CanvasEmptyActions } from "./CanvasEmptyActions";
import { CanvasUnownedTray } from "./CanvasUnownedTray";
import type { CanvasLod } from "./cards";
import { getNodeRenderer } from "./nodeRegistry";
import {
  layoutWorkspace,
  visibleFrameIds,
  visibleNodeIds,
  type LaidOutEdge,
  type LaidOutNode,
} from "./layout";
import { archivePlan } from "./plansApi";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";
import { filterPlansForCanvas } from "./planPaneMatch";
import { useFullPlans } from "./PlansProvider";
import type { CanvasElement, Milestone, OrchestratorRef, Plan, Section, Task } from "./types";
import { isDraggableNodeKind, useCanvasNodeDrag } from "./useCanvasNodeDrag";
import { useCanvasViewport } from "./useCanvasViewport";

export interface CanvasViewInnerProps {
  mode: "workspace" | "focus";
  plans: Plan[];
  loading?: boolean;
  error?: string | null;
  planId?: string;
  orchestratorPaneId?: string;
  panes?: Record<string, Pane>;
  onClose?: () => void;
  /** Talk-to-area composer needs an API config to POST to the owner pane. */
  api?: ApiConfig;
  /** Test hook: expand viewport culling margin. */
  viewportPadding?: number;
}

function lodForScale(scale: number): CanvasLod {
  const s = DEFAULT_CANVAS_SETTINGS;
  if (scale < s.zoomFarThreshold) return "far";
  if (scale < s.zoomNearThreshold) return "mid";
  return "near";
}

function nodeStyle(box: { x: number; y: number; width: number; height?: number }) {
  return {
    position: "absolute" as const,
    left: box.x,
    top: box.y,
    width: box.width,
    height: box.height,
  };
}

function edgeEndpoints(
  edge: LaidOutEdge,
  nodeById: Map<string, LaidOutNode>,
): { x1: number; y1: number; x2: number; y2: number } | null {
  const from = nodeById.get(edge.from);
  const to = nodeById.get(edge.to);
  if (!from || !to) return null;
  return {
    x1: from.box.x + from.box.width / 2,
    y1: from.box.y + from.box.height / 2,
    x2: to.box.x + to.box.width / 2,
    y2: to.box.y + to.box.height / 2,
  };
}

interface CanvasLookups {
  nodeById: Map<string, LaidOutNode>;
  sectionById: Map<string, Section>;
  taskById: Map<string, Task>;
  milestoneById: Map<string, Milestone>;
  elementById: Map<string, CanvasElement>;
}

function collectSections(sections: Section[], out: Map<string, Section>): void {
  for (const s of sections) {
    out.set(s.id, s);
    if (s.subsections.length) collectSections(s.subsections, out);
  }
}

export function CanvasViewInner({
  mode,
  plans,
  loading = false,
  error = null,
  planId,
  orchestratorPaneId,
  panes = {},
  onClose,
  api,
  viewportPadding,
}: CanvasViewInnerProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 800, height: 600 });

  const filteredPlans = useMemo(
    () =>
      filterPlansForCanvas(plans, mode, {
        planId,
        orchestratorPaneId,
        panes,
      }),
    [plans, mode, planId, orchestratorPaneId, panes],
  );

  const planById = useMemo(
    () => new Map(filteredPlans.map((p) => [p.id, p])),
    [filteredPlans],
  );

  const layout = useMemo(() => layoutWorkspace(filteredPlans), [filteredPlans]);

  // Build id→node/section/task/milestone maps ONCE per layout/plans change so the
  // pan/drag RAF path never rescans O(E*N) per edge or O(N) per visible node.
  const lookups = useMemo<CanvasLookups>(() => {
    const nodeById = new Map<string, LaidOutNode>();
    for (const frame of layout.frames) {
      for (const n of frame.nodes) nodeById.set(n.id, n);
    }
    const sectionById = new Map<string, Section>();
    const taskById = new Map<string, Task>();
    const milestoneById = new Map<string, Milestone>();
    const elementById = new Map<string, CanvasElement>();
    for (const plan of filteredPlans) {
      collectSections(plan.sections, sectionById);
      for (const task of plan.tasks) {
        taskById.set(task.id, task);
        for (const m of task.milestones) milestoneById.set(m.id, m);
      }
      for (const el of plan.elements) elementById.set(el.id, el);
    }
    return { nodeById, sectionById, taskById, milestoneById, elementById };
  }, [layout, filteredPlans]);

  // Resolve a node's model from the perf-fix memoized maps; keyed by where the
  // data lives, not by renderer. Unknown kinds resolve from free-form elements.
  const resolveModel = useCallback(
    (node: LaidOutNode): unknown => {
      switch (node.kind) {
        case "plan":
          return planById.get(node.id);
        case "section":
          return lookups.sectionById.get(node.id);
        case "task":
          return lookups.taskById.get(node.id);
        case "milestone":
          return lookups.milestoneById.get(node.id);
        default:
          return lookups.elementById.get(node.id);
      }
    },
    [planById, lookups],
  );

  const { transform, handlers, fitToView, isDragging, containerRef } = useCanvasViewport({
    width: size.width,
    height: size.height,
    panButton: "middle",
    ignoreSelector: "[data-canvas-draggable]",
  });

  const { dragOverride, draggingNodeId, onNodeMouseDown } = useCanvasNodeDrag({
    api,
    containerRef,
    transform,
    layout,
  });

  const handleArchive = useCallback(
    (planId: string) => {
      if (!api) return;
      void archivePlan(api, planId).catch(() => {});
    },
    [api],
  );

  const claimOwner = useMemo((): OrchestratorRef | undefined => {
    if (orchestratorPaneId) {
      return { pane_id: orchestratorPaneId, session_id: null, title: null };
    }
    const first = filteredPlans.find((p) => p.owner.pane_id);
    return first?.owner;
  }, [orchestratorPaneId, filteredPlans]);

  const effectiveBox = useCallback(
    (n: LaidOutNode) => {
      if (dragOverride && dragOverride.nodeId === n.id) {
        return { ...n.box, x: dragOverride.x, y: dragOverride.y };
      }
      return n.box;
    },
    [dragOverride],
  );

  const lod = lodForScale(transform.scale);
  const padding = viewportPadding ?? DEFAULT_CANVAS_SETTINGS.cullPadding;
  const canvasSize = useMemo(
    () => ({ width: size.width, height: size.height }),
    [size.width, size.height],
  );
  const frameIds = useMemo(
    () => new Set(visibleFrameIds(layout, transform, canvasSize, padding)),
    [layout, transform, canvasSize, padding],
  );
  const nodeIds = useMemo(
    () => new Set(visibleNodeIds(layout, transform, canvasSize, padding)),
    [layout, transform, canvasSize, padding],
  );

  const lastFitKey = useRef<string | null>(null);
  const fitKey = `${mode}:${filteredPlans.map((p) => p.id).join(",")}:${size.width}x${size.height}`;
  useEffect(() => {
    if (size.width <= 0 || size.height <= 0 || layout.bounds.width <= 0) return;
    if (lastFitKey.current === fitKey) return;
    lastFitKey.current = fitKey;
    fitToView(layout.bounds);
  }, [fitKey, fitToView, layout.bounds, size.width, size.height]);

  useEffect(() => {
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

  const worldW = Math.max(layout.bounds.width + 400, size.width);
  const worldH = Math.max(layout.bounds.height + 400, size.height);

  // World-space point under the viewport center drives the composer's targeting.
  const viewportCenter = useMemo(
    () => ({
      x: (size.width / 2 - transform.x) / transform.scale,
      y: (size.height / 2 - transform.y) / transform.scale,
    }),
    [size.width, size.height, transform.x, transform.y, transform.scale],
  );

  return (
    <div ref={shellRef} className="relative h-full w-full overflow-hidden" data-testid="canvas-view">
      {onClose && (
        <button
          type="button"
          data-testid="canvas-focus-close"
          onClick={onClose}
          className="absolute top-2 right-2 z-20 flex h-7 w-7 items-center justify-center rounded border cursor-pointer"
          style={{
            background: "var(--ct-bg)",
            borderColor: "var(--ct-border)",
            color: "var(--ct-fg)",
          }}
          title="Close plan canvas"
        >
          <X size={14} />
        </button>
      )}
      <div
        ref={containerRef}
        className="relative h-full w-full overflow-hidden"
        onMouseDown={handlers.onMouseDown}
        onWheel={handlers.onWheel}
        style={{
          cursor: isDragging ? "grabbing" : "grab",
          userSelect: "none",
          background:
            "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
        }}
      >
        {loading && (
          <div
            className="absolute inset-0 flex items-center justify-center text-sm"
            style={{ color: "var(--ct-fg)", opacity: "var(--ct-text-secondary-opacity)" }}
          >
            Loading plans…
          </div>
        )}
        {!loading && error && (
          <div
            className="absolute inset-0 flex items-center justify-center text-sm px-4 text-center"
            style={{ color: "var(--ct-error, #f87171)" }}
          >
            {error}
          </div>
        )}
        {!loading && !error && filteredPlans.length === 0 && (
          <div
            data-testid="canvas-empty"
            className="absolute inset-0 flex flex-col items-center justify-center text-sm px-6 text-center gap-4"
            style={{ color: "var(--ct-fg)", opacity: "var(--ct-text-secondary-opacity)" }}
          >
            <p>No active plans yet.</p>
            {mode === "workspace" && api && <CanvasEmptyActions api={api} />}
          </div>
        )}
        {!loading && filteredPlans.length > 0 && (
          <div
            style={{
              transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})`,
              transformOrigin: "0 0",
              width: worldW,
              height: worldH,
              position: "relative",
            }}
          >
            <svg
              className="pointer-events-none absolute inset-0"
              width={worldW}
              height={worldH}
              style={{ overflow: "visible" }}
            >
              {layout.frames.map((frame) => {
                if (!frameIds.has(frame.planId)) return null;
                return frame.edges.map((edge) => {
                  const pts = edgeEndpoints(edge, lookups.nodeById);
                  if (!pts) return null;
                  return (
                    <line
                      key={edge.id}
                      x1={pts.x1}
                      y1={pts.y1}
                      x2={pts.x2}
                      y2={pts.y2}
                      stroke="var(--ct-border-default)"
                      strokeWidth={1.5}
                      opacity={0.7}
                    />
                  );
                });
              })}
            </svg>
            {layout.frames.map((frame) => {
              if (!frameIds.has(frame.planId)) return null;
              const planNode: LaidOutNode = {
                id: frame.planId,
                kind: "plan",
                planId: frame.planId,
                box: frame.box,
                iteration: null,
                manual: false,
              };
              const rendered = getNodeRenderer("plan")({
                node: planNode,
                model: resolveModel(planNode),
                style: nodeStyle(frame.box),
                lod,
                onArchive: api ? () => handleArchive(frame.planId) : undefined,
              });
              return rendered ? <Fragment key={frame.planId}>{rendered}</Fragment> : null;
            })}
            {layout.frames.flatMap((frame) =>
              frame.nodes
                .filter((n) => nodeIds.has(n.id))
                .map((n) => {
                  const box = effectiveBox(n);
                  const draggable = isDraggableNodeKind(n.kind);
                  const rendered = getNodeRenderer(n.kind)({
                    node: { ...n, box },
                    model: resolveModel(n),
                    style: nodeStyle(box),
                    lod,
                    onMouseDown: draggable ? (e) => onNodeMouseDown(e, { ...n, box }) : undefined,
                    dragging: draggingNodeId === n.id,
                  });
                  return rendered ? <Fragment key={n.id}>{rendered}</Fragment> : null;
                }),
            )}
          </div>
        )}
      </div>
      {api && filteredPlans.length > 0 && (
        <CanvasComposer
          api={api}
          plans={filteredPlans}
          layout={layout}
          viewportCenter={viewportCenter}
          mode={mode}
        />
      )}
      {api && mode === "workspace" && <CanvasUnownedTray api={api} claimOwner={claimOwner} />}
    </div>
  );
}

export interface CanvasViewProps {
  mode: "workspace" | "focus";
  workspaceId?: string;
  planId?: string;
  orchestratorPaneId?: string;
  api: ApiConfig;
  onClose?: () => void;
}

export function CanvasView({ mode, planId, orchestratorPaneId, api, onClose }: CanvasViewProps) {
  const { plans, loading, error } = useFullPlans();
  const { state } = useMultiplexer();
  return (
    <CanvasViewInner
      mode={mode}
      plans={plans}
      loading={loading}
      error={error}
      planId={planId}
      orchestratorPaneId={orchestratorPaneId}
      panes={state.panes}
      onClose={onClose}
      api={api}
    />
  );
}
