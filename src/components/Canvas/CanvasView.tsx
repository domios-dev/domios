import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { ApiConfig, Pane, Workspace } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { CanvasComposer } from "./CanvasComposer";
import { resolveCanvasFocusNodeIds, type CanvasFocusPayload } from "./canvasFocusPayload";
import { CanvasEmptyActions } from "./CanvasEmptyActions";
import { CanvasToolbar } from "./CanvasToolbar";
import { CanvasMinimap } from "./CanvasMinimap";
import { CanvasEdges } from "./CanvasEdges";
import { CanvasTagLegend } from "./CanvasTagLegend";
import { CanvasUnownedTray } from "./CanvasUnownedTray";
import { OrchestratorGroup } from "./cards";
import {
  layoutWorkspace,
  layoutInputRevision,
  plansForLayout,
  buildLineageCatalog,
  visibleFrameIds,
  visibleNodeIds,
  type LaidOutNode,
} from "./layout";
import { archivePlan, fetchSpecSections } from "./plansApi";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";
import { crispRasterScale, nextWorld, worldThresholds, type World } from "./worlds";
import { assignedTaskIdsForPane } from "./planOpenFocus";
import { filterPlansForCanvas } from "./planPaneMatch";
import { useCanvasPlans } from "./PlansProvider";
import { useCanvasWorkspaceTarget } from "./canvasFocusStore";
import { isTagFilterActive, taskMatchesTagFilter, toggleTagFilter, type TagFilterSelection } from "./tagModel";
import type { PlanGhostModel } from "./cards";
import type { OrchestratorRef, Plan, Section, TagFacet } from "./types";
import { derivePlanWorkspaceScope } from "./planWorkspaceScope";
import { usePlanDrift } from "./usePlanDrift";
import { useCanvasViewport, formatLayerTransform } from "./useCanvasViewport";
import type { CanvasWorkspaceTarget } from "./canvasFocusStore";
import { canvasFitContextKey, resolveWorkspaceFocusNodeIds } from "./workspacePlanEntry";
import { CanvasFocusBreadcrumb } from "./CanvasFocusBreadcrumb";
import { CanvasPassiveRenderLayer } from "./CanvasPassiveRenderLayer";
import { CanvasRichOverlayLayer } from "./CanvasRichOverlayLayer";
import { CanvasPresenceLayer } from "./CanvasPresenceLayer";
import { CanvasSwarmCluster } from "./CanvasSwarmCluster";
import { CanvasWorldNodeList } from "./CanvasWorldNodeList";
import { CanvasWorldDecorations } from "./CanvasWorldDecorations";
import { overlayBudgetEnabled } from "./overlayBudgetFlag";
import { buildCanvasScene, pickCanvasSceneNodeAtScreenPoint } from "./scene";
import { substrateV2Enabled } from "./substrateFlag";
import { perfFixtureCount, buildPerfFixturePlans, realFixtureEnabled, realFixturePlans, fixtureSpecSectionsByPlan } from "./perfFixture";
import { flattenSections } from "./sectionRows";
import { lodForScale } from "./canvasView/models";
import { useCanvasLookups } from "./canvasView/useCanvasLookups";
import { useCanvasFlyTo } from "./canvasView/useCanvasFlyTo";
import { useCanvasOverlays } from "./canvasView/useCanvasOverlays";
import { useCanvasChangeSignal } from "./canvasView/useCanvasChangeSignal";

export type { ClusterCaptionModel, SpecSectionModel, TaskCardModel } from "./canvasView/models";

export interface CanvasViewInnerProps {
  mode: "workspace" | "focus";
  plans: Plan[];
  loading?: boolean;
  error?: string | null;
  planId?: string;
  orchestratorPaneId?: string;
  /** The pane that opened Plan mode — its assigned tasks get the initial camera. */
  focusPaneId?: string | null;
  panes?: Record<string, Pane>;
  workspaces?: Workspace[];
  focusedPaneId?: string | null;
  projectPath?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  onClose?: () => void;
  onOpenPane?: (paneId: string) => void;
  /** Talk-to-area composer needs an API config to POST to the owner pane. */
  api?: ApiConfig;
  /** Test hook: expand viewport culling margin. */
  viewportPadding?: number;
}

const EMPTY_WORKSPACES: Workspace[] = [];

export function CanvasViewInner({
  mode,
  plans,
  loading = false,
  error = null,
  planId,
  orchestratorPaneId,
  focusPaneId = null,
  panes = {},
  workspaces = EMPTY_WORKSPACES,
  focusedPaneId = null,
  projectPath = null,
  workspaceTarget = null,
  onClose,
  onOpenPane,
  api,
  viewportPadding,
}: CanvasViewInnerProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 800, height: 600 });

  const plansRevision = layoutInputRevision(plans);
  const planFilterRevision = plans
    .map((p) => [p.id, p.status, p.owner.pane_id ?? "", p.owner.session_id ?? ""].join(":"))
    .join("\0");
  const filteredPlans = useMemo(
    () => filterPlansForCanvas(plans, mode, { planId, orchestratorPaneId, panes }),
    [plans, plansRevision, planFilterRevision, mode, planId, orchestratorPaneId, panes],
  );

  const planById = useMemo(() => new Map(filteredPlans.map((p) => [p.id, p])), [filteredPlans]);
  const filteredPlanIds = useMemo(() => filteredPlans.map((p) => p.id), [filteredPlans]);
  const workspaceScopeByPlan = useMemo(
    () => new Map(filteredPlans.map((p) => [p.id, derivePlanWorkspaceScope(p, workspaces)])),
    [filteredPlans, workspaces],
  );
  const driftByPlan = usePlanDrift(api, filteredPlanIds);

  // Fetch the REAL linked Spec entity per plan (Spec=WHAT ⊥ Plan=HOW). The spec
  // node renders THESE sections, not the plan's — they are distinct entities. Dev
  // fixtures have no real Spec entity to fetch, so their spec sections are seeded
  // synthetically and the API fetch skips them.
  const fixtureSpecSections = useMemo(() => fixtureSpecSectionsByPlan(filteredPlans), [filteredPlans]);
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());
  useEffect(() => {
    if (!api) return;
    let active = true;
    const targets = filteredPlans.filter((p) => p.primary_spec_id && !fixtureSpecSections.has(p.id));
    if (!targets.length) return;
    void Promise.all(
      targets.map((p) =>
        fetchSpecSections(api, p.primary_spec_id as string)
          .then((sections) => [p.id, sections] as const)
          .catch(() => [p.id, [] as Section[]] as const),
      ),
    ).then((pairs) => {
      if (active) setFetchedSpecSections(new Map(pairs));
    });
    return () => {
      active = false;
    };
  }, [api, filteredPlans, fixtureSpecSections]);
  const specSectionsByPlan = useMemo(() => {
    if (!fixtureSpecSections.size) return fetchedSpecSections;
    const merged = new Map(fetchedSpecSections);
    for (const [k, v] of fixtureSpecSections) merged.set(k, v);
    return merged;
  }, [fetchedSpecSections, fixtureSpecSections]);
  // Per plan: spec_section_id → title, for the plan node's spec-reference bubbles.
  const specTitleByPlan = useMemo(() => {
    const m = new Map<string, Map<string, string>>();
    for (const [pid, secs] of specSectionsByPlan) {
      if (!secs.length) continue;
      m.set(pid, new Map(flattenSections(secs).map((s) => [s.id, s.title])));
    }
    return m;
  }, [specSectionsByPlan]);

  const { transform, handlers, fitToView, animateTo, animateToView, zoomBy, setTransform, isDragging, containerRef, layerRef } = useCanvasViewport({
    width: size.width,
    height: size.height,
    panButton: "middle",
    // §32.10 / worlds-dive: tasks/strides are tiny → deep zoom-drill needs wide range (to 1000%).
    maxScale: 10,
  });

  // §24.1: ONE behavior — no manual view modes. Node frames are fixed; content
  // adapts to camera zoom (content-LOD). The toolbar is camera-only.
  const [minimapOn, setMinimapOn] = useState(true);

  const layoutSourcePlans = plansForLayout(filteredPlans, specSectionsByPlan);
  const layoutRevision = layoutInputRevision(layoutSourcePlans, specSectionsByPlan);
  const lineageCatalog = useMemo(() => buildLineageCatalog(plans), [plansRevision]);
  const layout = useMemo(() => layoutWorkspace(layoutSourcePlans), [layoutRevision]);
  // Scale-independent: scene structure does not depend on camera zoom, so it
  // rebuilds only when the layout changes — not on every pan/zoom frame.
  const scene = useMemo(() => buildCanvasScene({ layout, plans: layoutSourcePlans }), [layout, layoutSourcePlans]);

  const lookups = useCanvasLookups(layout, filteredPlans, driftByPlan, panes);

  const resolveModel = useCallback(
    (node: LaidOutNode): unknown => {
      switch (node.kind) {
        case "plan":
          return planById.has(node.id)
            ? {
              ...planById.get(node.id)!,
              driftCount: driftByPlan.get(node.id)?.length ?? 0,
              workspaceScope: workspaceScopeByPlan.get(node.id),
              specTitleById: specTitleByPlan.get(node.id),
            }
            : undefined;
        case "spec": {
          const plan = planById.get(node.planId);
          const specSections = specSectionsByPlan.get(node.planId);
          // Render the REAL spec's sections (no tasks → coverage is a plan concept).
          return plan && specSections
            ? { ...plan, sections: specSections, tasks: [], workspaceScope: workspaceScopeByPlan.get(node.planId) }
            : plan
              ? { ...plan, workspaceScope: workspaceScopeByPlan.get(node.planId) }
              : undefined;
        }
        case "task":
          return lookups.taskModelById.get(node.id);
        case "plan-ghost": {
          const planId = node.id.startsWith("ghost:") ? node.id.slice(6) : node.id;
          const meta = lineageCatalog.get(planId);
          const ghost: PlanGhostModel = meta
            ? { planId: meta.id, title: meta.title, status: meta.status, sectionCount: meta.sectionCount, taskCount: meta.taskCount }
            : { planId, title: planId, status: "archived" };
          return ghost;
        }
        default:
          return lookups.elementById.get(node.id);
      }
    },
    [planById, lookups, specSectionsByPlan, specTitleByPlan, lineageCatalog, workspaceScopeByPlan],
  );

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

  const [tagFilter, setTagFilter] = useState<TagFilterSelection>(() => new Map());
  const onToggleTagFilter = useCallback((tag: string) => {
    setTagFilter((prev) => toggleTagFilter(prev, tag));
  }, []);
  const tagFilterActive = isTagFilterActive(tagFilter);
  const tagFilterMatchIds = useMemo(() => {
    if (!tagFilterActive) return null;
    const ids = new Set<string>();
    for (const [taskId, model] of lookups.taskModelById) {
      if (taskMatchesTagFilter(model.task.tags, tagFilter)) ids.add(taskId);
    }
    return ids;
  }, [tagFilter, tagFilterActive, lookups.taskModelById]);

  const lod = lodForScale(transform.scale);
  // §26 worlds: each node's content band is driven by its OWN appW (footprint ×
  // camera scale), not the global scale — a wide SPEC and a small TASK cross bands
  // at the right physical size. Hysteresis (prev band, ±10%) kills threshold flicker.
  const bandRef = useRef<Map<string, World>>(new Map());
  const worldByNode = useMemo(() => {
    const prev = bandRef.current;
    const next = new Map<string, World>();
    for (const frame of layout.frames) {
      for (const n of frame.nodes) {
        const t = worldThresholds(n.kind);
        next.set(n.id, nextWorld(prev.get(n.id), n.box.width * transform.scale, t));
      }
    }
    bandRef.current = next;
    return next;
  }, [layout, transform.scale]);
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
  const visibleNodes = useMemo(
    () => layout.frames.flatMap((frame) => frame.nodes.filter((n) => nodeIds.has(n.id))),
    [layout, nodeIds],
  );

  const [focusedNodeId, setFocusedNodeId] = useState<string | null>(null);

  const {
    blinkIds,
    highlight,
    fitCameraToNodes,
    focusOnNodes,
    onFlyToTask,
    onSectionClick,
    onSectionFrame,
    onCanvasFlyClick,
    onCopyLink,
    onFocusStride,
  } = useCanvasFlyTo({ lookups, planById, size, fitToView, animateToView, animateTo, setTransform });

  const { richOverlays, presenceOverlays, swarmRoster } = useCanvasOverlays({
    scene,
    transform,
    focusedNodeId,
    mode,
    planId,
    lookups,
    nodeIds,
    worldByNode,
  });

  // §25.3 DOMINANCE (focus-assist for the task you "fell into"): when one task fills
  // ≳60% of the viewport it ELEVATES (paint above + shadow, NO footprint change).
  // Siblings stay SOLID (§32.3 — depth, not fade). Viewport-relative, transient.
  const dominantNodeId = useMemo(() => {
    if (focusedNodeId) return null;
    let best: string | null = null;
    let bestArea = 0;
    for (const frame of layout.frames) {
      for (const n of frame.nodes) {
        if (n.kind !== "task" || n.box.width <= 0 || !nodeIds.has(n.id)) continue;
        const aw = n.box.width * transform.scale;
        const ah = n.box.height * transform.scale;
        if (aw < 0.6 * size.width && ah < 0.6 * size.height) continue;
        const area = aw * ah;
        if (area > bestArea) { bestArea = area; best = n.id; }
      }
    }
    return best;
  }, [layout, nodeIds, transform.scale, size.width, size.height, focusedNodeId]);

  const { pulsePlanIds, changeMarkers, flyToPlan } = useCanvasChangeSignal({
    filteredPlans,
    layout,
    transform,
    canvasSize,
    size,
    fitCameraToNodes,
  });

  // The active plan to land on at mount: an owned plan if any, else the first.
  const activePlanId = useMemo(
    () => (filteredPlans.find((p) => p.owner.pane_id) ?? filteredPlans[0])?.id ?? null,
    [filteredPlans],
  );

  const paneTaskIds = useMemo(
    () => (focusPaneId ? assignedTaskIdsForPane(filteredPlans, focusPaneId) : []),
    [focusPaneId, filteredPlans],
  );

  const workspaceFocusIds = useMemo(
    () => resolveWorkspaceFocusNodeIds(mode, workspaceTarget, filteredPlans),
    [mode, workspaceTarget, filteredPlans],
  );

  // §31: auto-fit runs ONLY on mount / when the mount CONTEXT changes (mode or the
  // focused pane) — NEVER on a data change. A new task/plan/stride updates nodes in
  // place; the camera transform stays byte-identical (no surprise pan/recenter).
  const fittedRef = useRef<string | null>(null);
  const fitContextKey = canvasFitContextKey(mode, focusPaneId, workspaceTarget);
  useEffect(() => {
    if (size.width <= 0 || size.height <= 0 || layout.bounds.width <= 0) return;
    if (fittedRef.current === fitContextKey) return;
    fittedRef.current = fitContextKey;
    // Task context -> fly to assigned task(s) on the full board. Normal Plan open
    // frames the active plan (§23.4), not fit-all (which lands at 5% on a big board).
    if (workspaceFocusIds.length) focusOnNodes(workspaceFocusIds);
    else if (paneTaskIds.length) focusOnNodes(paneTaskIds);
    else if (activePlanId) fitCameraToNodes([activePlanId]);
    else fitToView(layout.bounds);
  }, [fitContextKey, fitToView, fitCameraToNodes, focusOnNodes, workspaceFocusIds, paneTaskIds, activePlanId, layout.bounds, size.width, size.height]);

  // canvas-focus event (camera + blink). Backends emit {task_id, plan_id, pane_id}
  // (a task/plan node id == its id; a pane resolves to its assigned tasks). Optional
  // `scale` lands centered at an exact zoom for deterministic "go to id @ zoom" nav.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let active = true;
    void listen<CanvasFocusPayload>(
      "canvas-focus",
      (ev) => {
        const p = ev.payload;
        const ids = resolveCanvasFocusNodeIds(p, filteredPlans);
        if (ids.length) focusOnNodes(ids, p?.scale ?? undefined);
      },
    ).then((fn) => {
      if (!active) fn();
      else unlisten = fn;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [focusOnNodes, filteredPlans]);

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

  const legendFacets = useMemo(() => {
    const byKey = new Map<string, TagFacet>();
    for (const plan of filteredPlans) {
      for (const facet of plan.tag_facets ?? []) {
        if (!byKey.has(facet.key)) byKey.set(facet.key, facet);
      }
    }
    return [...byKey.values()];
  }, [filteredPlans]);

  const worldW = Math.max(layout.bounds.width + layout.bounds.x + 400, size.width);
  const worldH = Math.max(layout.bounds.height + layout.bounds.y + 400, size.height);

  const viewportCenter = useMemo(
    () => ({
      x: (size.width / 2 - transform.x) / transform.scale,
      y: (size.height / 2 - transform.y) / transform.scale,
    }),
    [size.width, size.height, transform.x, transform.y, transform.scale],
  );

  const fitView = useCallback(() => fitToView(layout.bounds), [fitToView, layout.bounds]);
  const resetView = useCallback(() => {
    const cx = layout.bounds.x + layout.bounds.width / 2;
    const cy = layout.bounds.y + layout.bounds.height / 2;
    setTransform({ scale: 1, x: size.width / 2 - cx, y: size.height / 2 - cy });
  }, [layout.bounds, size.width, size.height, setTransform]);
  const fitNode = useCallback(
    (nodeId: string) => {
      const box = lookups.nodeById.get(nodeId)?.box;
      if (box) fitToView(box);
    },
    [lookups, fitToView],
  );

  // §23.4 camera presets + focus gesture — camera-only (no node modes, §24.1).
  // Content reveals itself by zoom: Overview = zoom out (compact), Read = zoom in.
  const overviewPreset = useCallback(() => {
    setFocusedNodeId(null);
    fitToView(layout.bounds);
  }, [fitToView, layout.bounds]);
  const readPreset = useCallback(() => {
    if (activePlanId) {
      setFocusedNodeId(activePlanId);
      fitCameraToNodes([activePlanId]);
    }
  }, [activePlanId, fitCameraToNodes]);
  const focusNodeCamera = useCallback(
    (id: string) => {
      setFocusedNodeId(id);
      fitCameraToNodes([id]);
    },
    [fitCameraToNodes],
  );

  useEffect(() => {
    if (!focusedNodeId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setFocusedNodeId(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focusedNodeId]);

  const onWorldDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      const rect = containerRef.current?.getBoundingClientRect();
      const hit = substrateV2Enabled() && rect
        ? pickCanvasSceneNodeAtScreenPoint(scene, { x: e.clientX - rect.left, y: e.clientY - rect.top }, transform)
        : null;
      const el = hit ? null : (e.target as HTMLElement).closest("[data-plan-id],[data-task-id]") as HTMLElement | null;
      const id = hit?.id ?? el?.getAttribute("data-plan-id") ?? el?.getAttribute("data-task-id");
      if (id) focusNodeCamera(id);
    },
    [containerRef, focusNodeCamera, scene, transform],
  );

  const focusedTitle = focusedNodeId
    ? planById.get(focusedNodeId)?.title ?? lookups.taskById.get(focusedNodeId)?.name ?? null
    : null;

  return (
    <div ref={shellRef} className="relative h-full w-full overflow-hidden" data-testid="canvas-view" data-overlay-budget={overlayBudgetEnabled() ? "true" : "false"}>
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
        onClick={onCanvasFlyClick}
        style={{
          cursor: isDragging ? "grabbing" : "default",
          userSelect: "none",
          touchAction: "none",
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
          <CanvasPassiveRenderLayer scene={scene} transform={transform} width={size.width} height={size.height} hostRef={containerRef}
            focusedNodeId={focusedNodeId} dominantNodeId={dominantNodeId} hoveredTask={hoveredTask} highlight={highlight}
            tagFilterMatchIds={tagFilterMatchIds} blinkIds={blinkIds} pulsePlanIds={pulsePlanIds} tagFilterActive={tagFilterActive} />
        )}
        {!loading && filteredPlans.length > 0 && (
          <CanvasRichOverlayLayer
            overlays={richOverlays}
            nodeById={lookups.nodeById}
            resolveModel={resolveModel}
            onOpenPane={onOpenPane}
            onHoverTask={onHoverTask}
            onArchive={api ? handleArchive : undefined}
            onFitNode={fitNode}
            onCopyLink={onCopyLink}
            onFocusStride={onFocusStride}
            onSectionClick={onSectionClick}
            onSectionFrame={onSectionFrame}
          />
        )}
        {!loading && filteredPlans.length > 0 && (
          <CanvasPresenceLayer overlays={presenceOverlays} onFlyToTask={onFlyToTask} onHoverTask={onHoverTask} />
        )}
        {!loading && !error && filteredPlans.length > 0 && (
          <CanvasSwarmCluster roster={swarmRoster} onFlyToTask={onFlyToTask} reserveTopRight={!!onClose} />
        )}
        {!loading && filteredPlans.length > 0 && (
          <div
            ref={layerRef}
            data-testid="canvas-world-layer"
            data-layout-revision={layoutRevision}
            onDoubleClick={onWorldDoubleClick}
            style={{
              transform: formatLayerTransform(transform),
              transformOrigin: "0 0",
              width: worldW,
              height: worldH,
              position: "relative",
              zIndex: 2,
            }}
          >
            {layout.groups.map((group) => (
              <OrchestratorGroup
                key={group.ownerKey}
                group={group}
                style={{ left: group.box.x, top: group.box.y, width: group.box.width, height: group.box.height }}
                scale={transform.scale}
                onOpen={onOpenPane}
              />
            ))}
            <svg className="pointer-events-none absolute inset-0" width={worldW} height={worldH} style={{ overflow: "visible" }}>
              <defs>
                <marker
                  id="ct-plan-lineage-arrow"
                  markerWidth="8"
                  markerHeight="8"
                  refX="7"
                  refY="4"
                  orient="auto"
                >
                  <polygon points="0,0 8,4 0,8" fill="var(--ct-border-focus)" />
                </marker>
              </defs>
              {layout.frames.map((frame) =>
                frameIds.has(frame.planId) ? (
                  <CanvasEdges
                    key={frame.planId}
                    edges={frame.edges}
                    nodeById={lookups.nodeById}
                    visibleIds={nodeIds}
                    hoveredTask={hoveredTask}
                    highlight={highlight}
                    tagFilterActive={tagFilterActive}
                    tagFilterMatchIds={tagFilterMatchIds}
                  />
                ) : null,
              )}
              <CanvasEdges
                edges={layout.crossEdges ?? []}
                nodeById={lookups.nodeById}
                visibleIds={nodeIds}
                hoveredTask={hoveredTask}
                highlight={highlight}
                tagFilterActive={tagFilterActive}
                tagFilterMatchIds={tagFilterMatchIds}
              />
            </svg>
            <CanvasWorldNodeList
              nodes={visibleNodes}
              lod={lod}
              worldByNode={worldByNode}
              crispRasterScale={crispRasterScale}
              transformScale={transform.scale}
              dominantNodeId={dominantNodeId}
              resolveModel={resolveModel}
              onOpenPane={onOpenPane}
              onHoverTask={onHoverTask}
              onArchive={api ? handleArchive : undefined}
              onFitNode={fitNode}
              onCopyLink={onCopyLink}
              onFocusStride={onFocusStride}
              onSectionClick={onSectionClick}
              onSectionFrame={onSectionFrame}
              tagFilterActive={tagFilterActive}
              tagFilterMatchIds={tagFilterMatchIds}
            />
            <CanvasWorldDecorations
              nodeById={lookups.nodeById}
              nodeIds={nodeIds}
              hoveredTask={hoveredTask}
              highlight={highlight}
              tagFilterMatchIds={tagFilterMatchIds}
              blinkIds={blinkIds}
              pulsePlanIds={pulsePlanIds}
            />
          </div>
        )}
        {/* §31 off-screen change marker: a directional dot at the viewport edge
            toward an off-screen live change. Click = USER-initiated fly-there. */}
        {changeMarkers.map((m) => (
          <button
            key={`marker-${m.planId}`}
            type="button"
            data-testid="canvas-change-marker"
            title="A plan changed off-screen — click to view"
            onClick={() => flyToPlan(m.planId)}
            className="ct-canvas-edge-marker"
            style={{ position: "absolute", left: m.x, top: m.y, transform: "translate(-50%, -50%)", zIndex: 25 }}
          />
        ))}
      </div>
      {!loading && !error && filteredPlans.length > 0 && (
        <CanvasToolbar
          scale={transform.scale}
          onZoomIn={() => zoomBy(0.2)}
          onZoomOut={() => zoomBy(-0.2)}
          onFit={fitView}
          onReset={resetView}
          onOverview={overviewPreset}
          onRead={readPreset}
          onToggleMinimap={() => setMinimapOn((v) => !v)}
          minimapOn={minimapOn}
        />
      )}
      {focusedNodeId && <CanvasFocusBreadcrumb title={focusedTitle} onBack={() => setFocusedNodeId(null)} />}
      {legendFacets.length > 0 && !loading && !error && filteredPlans.length > 0 && (
        <CanvasTagLegend facets={legendFacets} filter={tagFilter} onToggleTag={onToggleTagFilter} />
      )}
      {minimapOn && !loading && !error && filteredPlans.length > 0 && (
        <CanvasMinimap bounds={layout.bounds} nodes={[...lookups.nodeById.values()]} transform={transform} viewport={size} />
      )}
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
  focusPaneId?: string | null;
  api: ApiConfig;
  onClose?: () => void;
}

export function CanvasView({ mode, planId, orchestratorPaneId, focusPaneId, api, onClose }: CanvasViewProps) {
  const { state, setFocusedPane } = useMultiplexer();
  const activeWs = state.workspaces.find((ws) => ws.id === state.active_workspace_id);
  const projectPath = activeWs?.cwd ?? null;
  const workspaceTarget = useCanvasWorkspaceTarget();
  const { plans: livePlans, loading, error } = useCanvasPlans(mode, projectPath);
  // Dev harness: real exported plan, or a synthetic N-task plan, when flagged.
  const perfN = perfFixtureCount();
  const useReal = realFixtureEnabled();
  const plans = useMemo(
    () => (useReal ? realFixturePlans() : perfN > 0 ? buildPerfFixturePlans(perfN) : livePlans),
    [useReal, perfN, livePlans],
  );
  return (
    <CanvasViewInner
      mode={mode}
      plans={plans}
      loading={loading}
      error={error}
      planId={planId}
      orchestratorPaneId={orchestratorPaneId}
      focusPaneId={focusPaneId}
      panes={state.panes}
      workspaces={state.workspaces}
      focusedPaneId={state.focused_pane_id}
      projectPath={projectPath}
      workspaceTarget={mode === "workspace" ? workspaceTarget : null}
      onClose={onClose}
      onOpenPane={setFocusedPane}
      api={api}
    />
  );
}
