import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { ApiConfig, Pane } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { CanvasComposer } from "./CanvasComposer";
import { CanvasEmptyActions } from "./CanvasEmptyActions";
import { CanvasUnownedTray } from "./CanvasUnownedTray";
import type { CanvasLod, MilestoneBubbleModel } from "./cards";
import { OrchestratorGroup } from "./cards";
import { getNodeRenderer } from "./nodeRegistry";
import { bezierPath, resolveEdgeEnds } from "./edges";
import { milestoneColorMap } from "./milestoneColors";
import { layoutWorkspace, visibleFrameIds, visibleNodeIds, type LaidOutNode } from "./layout";
import { archivePlan } from "./plansApi";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";
import { filterPlansForCanvas } from "./planPaneMatch";
import { useCanvasPlans } from "./PlansProvider";
import type { CanvasElement, OrchestratorRef, Plan, Task } from "./types";
import { useCanvasViewport } from "./useCanvasViewport";

export interface CanvasViewInnerProps {
  mode: "workspace" | "focus";
  plans: Plan[];
  loading?: boolean;
  error?: string | null;
  planId?: string;
  orchestratorPaneId?: string;
  panes?: Record<string, Pane>;
  focusedPaneId?: string | null;
  projectPath?: string | null;
  onClose?: () => void;
  onOpenPane?: (paneId: string) => void;
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
    minHeight: box.height,
  };
}

interface CanvasLookups {
  nodeById: Map<string, LaidOutNode>;
  taskById: Map<string, Task>;
  milestoneModelById: Map<string, MilestoneBubbleModel>;
  elementById: Map<string, CanvasElement>;
}

interface CanvasNodeCardProps {
  node: LaidOutNode;
  model: unknown;
  lod: CanvasLod;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
  onArchive?: () => void;
}

const CanvasNodeCard = memo(
  function CanvasNodeCard({ node, model, lod, onOpenPane, onHoverTask, onArchive }: CanvasNodeCardProps) {
    const rendered = getNodeRenderer(node.kind)({
      node,
      model,
      style: nodeStyle(node.box),
      lod,
      onOpenPane,
      onHoverTask,
      onArchive,
    });
    return rendered ? <Fragment key={node.id}>{rendered}</Fragment> : null;
  },
  (prev, next) => {
    if (prev.node.id !== next.node.id) return false;
    if (prev.lod !== next.lod) return false;
    if (prev.model !== next.model) return false;
    const a = prev.node.box;
    const b = next.node.box;
    return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
  },
);

export function CanvasViewInner({
  mode,
  plans,
  loading = false,
  error = null,
  planId,
  orchestratorPaneId,
  panes = {},
  focusedPaneId = null,
  projectPath = null,
  onClose,
  onOpenPane,
  api,
  viewportPadding,
}: CanvasViewInnerProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 800, height: 600 });

  const filteredPlans = useMemo(
    () => filterPlansForCanvas(plans, mode, { planId, orchestratorPaneId, panes }),
    [plans, mode, planId, orchestratorPaneId, panes],
  );

  const planById = useMemo(() => new Map(filteredPlans.map((p) => [p.id, p])), [filteredPlans]);
  const layout = useMemo(() => layoutWorkspace(filteredPlans), [filteredPlans]);

  const lookups = useMemo<CanvasLookups>(() => {
    const nodeById = new Map<string, LaidOutNode>();
    for (const frame of layout.frames) for (const n of frame.nodes) nodeById.set(n.id, n);
    const taskById = new Map<string, Task>();
    const milestoneModelById = new Map<string, MilestoneBubbleModel>();
    const elementById = new Map<string, CanvasElement>();
    for (const plan of filteredPlans) {
      const colors = milestoneColorMap(plan);
      for (const task of plan.tasks) {
        taskById.set(task.id, task);
        for (const m of task.milestones) {
          milestoneModelById.set(m.id, { milestone: m, color: colors.get(m.kind) ?? "var(--ct-border-focus)" });
        }
      }
      for (const el of plan.elements) elementById.set(el.id, el);
    }
    return { nodeById, taskById, milestoneModelById, elementById };
  }, [layout, filteredPlans]);

  const resolveModel = useCallback(
    (node: LaidOutNode): unknown => {
      switch (node.kind) {
        case "plan":
          return planById.get(node.id);
        case "spec":
          return planById.get(node.planId);
        case "task":
          return lookups.taskById.get(node.id);
        case "milestone":
          return lookups.milestoneModelById.get(node.id);
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
  });

  const handleArchive = useCallback(
    (id: string) => {
      if (!api) return;
      void archivePlan(api, id).catch(() => {});
    },
    [api],
  );

  const claimOwner = useMemo((): OrchestratorRef | undefined => {
    if (orchestratorPaneId) return { pane_id: orchestratorPaneId, session_id: null, title: null };
    const owned = filteredPlans.find((p) => p.owner.pane_id);
    if (owned?.owner.pane_id) return owned.owner;
    if (focusedPaneId) {
      const pane = panes[focusedPaneId];
      return { pane_id: focusedPaneId, session_id: null, title: pane?.title ?? null };
    }
    return undefined;
  }, [orchestratorPaneId, filteredPlans, focusedPaneId, panes]);

  const [hoveredTask, setHoveredTask] = useState<string | null>(null);
  const onHoverTask = useCallback((id: string | null) => setHoveredTask(id), []);

  const lod = lodForScale(transform.scale);
  const padding = viewportPadding ?? DEFAULT_CANVAS_SETTINGS.cullPadding;
  const canvasSize = useMemo(() => ({ width: size.width, height: size.height }), [size.width, size.height]);
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

  // canvas-focus event (camera + blink). Backend lane emits target ids; we fit to
  // their bbox and blink them. Stub-safe: no-op until the event carries targets.
  const [blinkIds, setBlinkIds] = useState<Set<string>>(() => new Set());
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let active = true;
    void listen<{ node_ids?: string[]; plan_ids?: string[] }>("canvas-focus", (ev) => {
      const ids = [...(ev.payload?.node_ids ?? []), ...(ev.payload?.plan_ids ?? [])];
      const boxes = ids.map((id) => lookups.nodeById.get(id)?.box).filter(Boolean) as { x: number; y: number; width: number; height: number }[];
      if (boxes.length) {
        const minX = Math.min(...boxes.map((b) => b.x));
        const minY = Math.min(...boxes.map((b) => b.y));
        const maxX = Math.max(...boxes.map((b) => b.x + b.width));
        const maxY = Math.max(...boxes.map((b) => b.y + b.height));
        fitToView({ x: minX, y: minY, width: maxX - minX, height: maxY - minY });
      }
      setBlinkIds(new Set(ids));
    }).then((fn) => {
      if (!active) fn();
      else unlisten = fn;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [fitToView, lookups]);

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

  const worldW = Math.max(layout.bounds.width + layout.bounds.x + 400, size.width);
  const worldH = Math.max(layout.bounds.height + layout.bounds.y + 400, size.height);

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
          style={{ background: "var(--ct-bg)", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
          title="Close plan canvas"
        >
          <X size={14} />
        </button>
      )}
      <div
        ref={containerRef}
        className="relative h-full w-full overflow-hidden"
        onMouseDown={handlers.onMouseDown}
        style={{
          cursor: isDragging ? "grabbing" : "default",
          userSelect: "none",
          background: "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
        }}
      >
        {loading && (
          <div className="absolute inset-0 flex items-center justify-center text-sm" style={{ color: "var(--ct-fg)", opacity: "var(--ct-text-secondary-opacity)" }}>
            Loading plans…
          </div>
        )}
        {!loading && error && (
          <div className="absolute inset-0 flex items-center justify-center text-sm px-4 text-center" style={{ color: "var(--ct-error, #f87171)" }}>
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
            {mode === "workspace" && api && <CanvasEmptyActions api={api} projectPath={projectPath} />}
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
            {layout.groups.map((group) => (
              <OrchestratorGroup
                key={group.ownerKey}
                group={group}
                style={{ left: group.box.x, top: group.box.y, width: group.box.width, height: group.box.height }}
                onOpen={onOpenPane}
              />
            ))}
            <svg className="pointer-events-none absolute inset-0" width={worldW} height={worldH} style={{ overflow: "visible" }}>
              {layout.frames.map((frame) => {
                if (!frameIds.has(frame.planId)) return null;
                return frame.edges.map((edge) => {
                  const ends = resolveEdgeEnds(edge, lookups.nodeById);
                  if (!ends) return null;
                  const chain = edge.kind === "milestone-chain";
                  const sectionTask = edge.kind === "section-task";
                  // De-emphasized by default; hovering a task lifts its own edges
                  // and dims the rest so the row→task mapping reads at a glance.
                  const related = hoveredTask != null && (edge.from === hoveredTask || edge.to === hoveredTask);
                  const dimmed = hoveredTask != null && !related;
                  const baseW = sectionTask ? 1.4 : chain ? 1 : 1.2;
                  const baseO = sectionTask ? 0.42 : chain ? 0.3 : 0.34;
                  return (
                    <g key={edge.id}>
                      <path
                        d={bezierPath(ends.a, ends.b, DEFAULT_CANVAS_SETTINGS.edgeCurve)}
                        fill="none"
                        stroke="var(--ct-border-focus)"
                        strokeWidth={related ? 2.4 : baseW}
                        strokeLinecap="round"
                        opacity={related ? 0.92 : dimmed ? 0.1 : baseO}
                      />
                      {sectionTask && (
                        <circle
                          cx={ends.a.x}
                          cy={ends.a.y}
                          r={related ? 4 : 3}
                          fill="var(--ct-border-focus)"
                          opacity={dimmed ? 0.25 : 0.9}
                        />
                      )}
                    </g>
                  );
                });
              })}
            </svg>
            {layout.frames.flatMap((frame) =>
              frame.nodes
                .filter((n) => nodeIds.has(n.id))
                .map((n) => (
                  <CanvasNodeCard
                    key={n.id}
                    node={n}
                    model={resolveModel(n)}
                    lod={lod}
                    onOpenPane={onOpenPane}
                    onHoverTask={onHoverTask}
                    onArchive={n.kind === "plan" && api ? () => handleArchive(n.id) : undefined}
                  />
                )),
            )}
            {[...blinkIds].map((id) => {
              const box = lookups.nodeById.get(id)?.box;
              if (!box || !nodeIds.has(id)) return null;
              return (
                <div
                  key={`blink-${id}`}
                  className="ct-canvas-blink pointer-events-none absolute"
                  style={{
                    left: box.x - 4,
                    top: box.y - 4,
                    width: box.width + 8,
                    height: box.height + 8,
                    borderRadius: "var(--ct-radius-lg)",
                    border: "2px solid var(--ct-border-focus)",
                  }}
                />
              );
            })}
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
          panes={panes}
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
  const { state, setFocusedPane } = useMultiplexer();
  const activeWs = state.workspaces.find((ws) => ws.id === state.active_workspace_id);
  const projectPath = activeWs?.cwd ?? null;
  const { plans, loading, error } = useCanvasPlans(mode, projectPath);
  return (
    <CanvasViewInner
      mode={mode}
      plans={plans}
      loading={loading}
      error={error}
      planId={planId}
      orchestratorPaneId={orchestratorPaneId}
      panes={state.panes}
      focusedPaneId={state.focused_pane_id}
      projectPath={projectPath}
      onClose={onClose}
      onOpenPane={setFocusedPane}
      api={api}
    />
  );
}
