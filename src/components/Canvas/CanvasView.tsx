import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { ApiConfig, Tab, Team, Workspace } from "../../lib/types";
import { useMultiplexer, useMuxState } from "../../store/multiplexer";
import { resolveCanvasFocusNodeIds, type CanvasFocusPayload } from "./canvasFocusPayload";
import { CanvasEmptyActions } from "./CanvasEmptyActions";
import { CanvasToolbar } from "./CanvasToolbar";
import { CanvasMinimap } from "./CanvasMinimap";
import { CanvasTagLegend } from "./CanvasTagLegend";
import {
  layoutWorkspace,
  layoutInputRevision,
  plansForLayout,
  buildLineageCatalog,
  visibleNodeIds,
  type LaidOutNode,
} from "./layout";
import { fetchSpecSections } from "./plansApi";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";
import { nextWorld, worldThresholds, type World } from "./worlds";
import { assignedTaskIdsForTab } from "./planOpenFocus";
import { filterPlansForCanvas } from "./planTabMatch";
import { useCanvasPlans } from "./PlansProvider";
import { useCanvasWorkspaceTarget } from "./canvasFocusStore";
import { isTagFilterActive, taskMatchesTagFilter, toggleTagFilter, type TagFilterSelection } from "./tagModel";
import type { PlanGhostModel } from "./canvasView/models";
import type { Plan, Section, TagFacet } from "./types";
import { derivePlanWorkspaceScope } from "./planWorkspaceScope";
import { usePlanDrift } from "./usePlanDrift";
import { useCanvasViewport, paintLayer, CANVAS_MIN_SCALE, CANVAS_MAX_SCALE, type ViewportTransform } from "./useCanvasViewport";
import { buildSectionGroups, resolveSceneLod } from "./canvasRender/sceneLod";
import type { CanvasWorkspaceTarget } from "./canvasFocusStore";
import { canvasFitContextKey, resolveWorkspaceFocusNodeIds } from "./workspacePlanEntry";
import { CanvasPresenceLayer } from "./CanvasPresenceLayer";
import { CanvasDetailOverlay } from "./CanvasDetailOverlay";
import { CanvasWorldCanvas, type CanvasRenderApi } from "./canvasRender/CanvasWorldCanvas";
import type { GroupGlyph } from "./canvasRender/drawScene";
import { CanvasSwarmCluster } from "./CanvasSwarmCluster";
import { buildCanvasScene } from "./scene";
import { perfFixtureCount, perfFixtureProfile, buildPerfFixturePlans, buildDoneHeavyPerfFixturePlans, buildCardMetadataFixturePlans, buildCardMetadataFixtureTabs, buildPerfFixtureTabs, realFixtureEnabled, realFixturePlans, fixtureSpecSectionsByPlan, buildPlanWithKnownTasksFixturePlans, buildTabTreeWithStatusesFixturePlans, buildTabTreeWithStatusesFixtureTabs, buildMidStreamChatTranscriptFixturePlans, buildMidStreamChatTranscriptFixtureTabs } from "./perfFixture";
import { flattenSections } from "./sectionRows";
import { buildTaskPresenceFingerprint, useCanvasLookups } from "./canvasView/useCanvasLookups";
import { useCanvasFlyTo } from "./canvasView/useCanvasFlyTo";
import { useCanvasNavHistory, type NavEntry } from "./canvasView/useCanvasNavHistory";
import { rollUpSectionAgents, type SectionAgentSource } from "./sectionAgents";
import type { PresenceAgent } from "./presence/identity";
import { useCanvasOverlays } from "./canvasView/useCanvasOverlays";
import { useCanvasChangeSignal } from "./canvasView/useCanvasChangeSignal";
import { useStableSet } from "./canvasView/useStableSet";
import { useCanvasFrameProbe } from "./canvasView/useCanvasFrameProbe";
import { bumpCanvasChurn } from "./canvasChurn";
import { PlanViewShell } from "./PlanViewShell";
import { usePlanTargetNavigation } from "./teamViewNavigation";
import { selectCanvasPlans } from "./canvasPlanSelection";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "./planView/PlanFetchErrorState";
import { planFetchShowsData } from "./planView/planViewFetchGate";
import { revealAssignedAgentTab } from "./planRevealAgentTab";
import { FlyToActiveControl } from "./FlyToActiveControl";
import { selectActiveWork } from "./planView/activeWork";

export type { TaskCardModel } from "./canvasView/models";

export interface CanvasViewInnerProps {
  mode: "workspace" | "focus";
  plans: Plan[];
  loading?: boolean;
  reconnecting?: boolean;
  error?: string | null;
  onRetry?: () => void;
  planId?: string;
  orchestratorTabId?: string;
  /** The tab that opened Plan mode — its assigned tasks get the initial camera. */
  focusTabId?: string | null;
  tabs?: Record<string, Tab>;
  workspaces?: Workspace[];
  /** Current workspace; workspace mode scopes plans to those bound here. */
  workspaceId?: string;
  teams?: Team[];
  projectPath?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  onClose?: () => void;
  onOpenTab?: (tabId: string) => void;
  /** Talk-to-area composer needs an API config to POST to the owner tab. */
  api?: ApiConfig;
  /** Test hook: expand viewport culling margin. */
  viewportPadding?: number;
}

const EMPTY_WORKSPACES: Workspace[] = [];

function sameWorldMap(a: Map<string, World>, b: Map<string, World>): boolean {
  if (a.size !== b.size) return false;
  for (const [k, v] of b) if (a.get(k) !== v) return false;
  return true;
}

export function CanvasViewInner({
  mode,
  plans,
  loading = false,
  reconnecting = false,
  error = null,
  onRetry,
  planId,
  orchestratorTabId,
  focusTabId = null,
  tabs = {},
  workspaces = EMPTY_WORKSPACES,
  workspaceId,
  teams,
  projectPath = null,
  workspaceTarget = null,
  onClose,
  onOpenTab,
  api,
  viewportPadding,
}: CanvasViewInnerProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 800, height: 600 });
  useEffect(() => {
    bumpCanvasChurn("canvasCommits");
  });

  const plansRevision = layoutInputRevision(plans);
  const planFilterRevision = plans
    .map((p) => [p.id, p.status, p.owner.tab_id ?? "", p.owner.session_id ?? ""].join(":"))
    .join("\0");
  const teamId = mode === "workspace" ? workspaceTarget?.teamId : undefined;
  const filteredPlans = useMemo(
    () => filterPlansForCanvas(plans, mode, { planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams }),
    [plans, plansRevision, planFilterRevision, mode, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams],
  );

  const activeWork = useMemo(
    () => selectActiveWork(filteredPlans, tabs),
    [filteredPlans, tabs],
  );
  const staleActiveTaskIds = useMemo(
    () => new Set(activeWork.entries.filter((e) => e.stale).map((e) => e.task.id)),
    [activeWork],
  );

  const planById = useMemo(() => new Map(filteredPlans.map((p) => [p.id, p])), [filteredPlans]);
  const filteredPlanIds = useMemo(() => filteredPlans.map((p) => p.id), [filteredPlans]);
  const workspaceScopeByPlan = useMemo(
    () => new Map(filteredPlans.map((p) => [p.id, derivePlanWorkspaceScope(p, workspaces)])),
    [filteredPlans, workspaces],
  );
  const driftByPlan = usePlanDrift(api, filteredPlanIds);

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
  const sectionTitleById = useMemo(() => {
    const m = new Map<string, string>();
    for (const p of filteredPlans) {
      for (const s of flattenSections(p.sections)) {
        const t = s.title?.trim();
        if (t) m.set(s.id, t);
      }
    }
    return m;
  }, [filteredPlans]);

  // Per plan: spec_section_id → title, for the plan node's spec-reference bubbles.
  const specTitleByPlan = useMemo(() => {
    const m = new Map<string, Map<string, string>>();
    for (const [pid, secs] of specSectionsByPlan) {
      if (!secs.length) continue;
      m.set(pid, new Map(flattenSections(secs).map((s) => [s.id, s.title])));
    }
    return m;
  }, [specSectionsByPlan]);

  // on every camera paint so the canvas re-blits in lockstep with the gesture.
  const drawRequestRef = useRef<(() => void) | null>(null);
  const canvasApiRef = useRef<CanvasRenderApi | null>(null);
  const hoverRafRef = useRef<number | null>(null);
  const onPaintImpl = useRef<(t: ViewportTransform) => void>(() => {});
  const { transform, handlers, fitToView, animateTo, animateToView, zoomBy, setTransform, isDragging, containerRef, layerRef, transformRef } = useCanvasViewport({
    width: size.width,
    height: size.height,
    panButton: "middle",
    minScale: CANVAS_MIN_SCALE,
    maxScale: CANVAS_MAX_SCALE,
    onPaint: (t) => onPaintImpl.current(t),
  });
  onPaintImpl.current = (t) => {
    bumpCanvasChurn("cameraPaints");
    paintLayer(layerRef.current, t);
    drawRequestRef.current?.();
  };
  const paintWorldLayer = useCallback((t: ViewportTransform) => {
    paintLayer(layerRef.current, t);
  }, [layerRef]);

  useLayoutEffect(() => {
    paintLayer(layerRef.current, transformRef.current);
  }, [transform, layerRef, transformRef]);

  // adapts to camera zoom (content-LOD). The toolbar is camera-only.
  const [minimapOn, setMinimapOn] = useState(true);

  const layoutSourcePlans = useMemo(() => plansForLayout(filteredPlans, specSectionsByPlan), [filteredPlans, specSectionsByPlan]);
  const layoutRevision = useMemo(() => layoutInputRevision(layoutSourcePlans, specSectionsByPlan), [layoutSourcePlans, specSectionsByPlan]);
  const lineageCatalog = useMemo(() => buildLineageCatalog(plans), [plansRevision]);
  const layout = useMemo(() => layoutWorkspace(layoutSourcePlans, undefined, sectionTitleById), [layoutRevision, sectionTitleById]);
  // rebuilds only when the layout changes — not on every pan/zoom frame.
  const scene = useMemo(() => {
    bumpCanvasChurn("sceneBuilds");
    return buildCanvasScene({ layout, plans: layoutSourcePlans });
  }, [layout, layoutSourcePlans]);

  const tabsContentKey = useMemo(() => buildTaskPresenceFingerprint(filteredPlans, tabs), [filteredPlans, tabs]);
  const lookups = useCanvasLookups(layout, filteredPlans, driftByPlan, tabs, tabsContentKey);

  // deduped — a section shows who's on it RIGHT NOW. Camera-decoupled (presence + data).
  const sectionAgentsByPlan = useMemo(() => {
    const out = new Map<string, Map<string, PresenceAgent[]>>();
    for (const plan of filteredPlans) {
      const src = new Map<string, SectionAgentSource>();
      for (const t of plan.tasks) {
        const p = lookups.taskModelById.get(t.id)?.presence;
        if (p) src.set(t.id, { agents: p.agents, live: t.status === "in_progress" });
      }
      const bySection = rollUpSectionAgents(plan.tasks, src);
      if (bySection.size) out.set(plan.id, bySection);
    }
    return out;
  }, [filteredPlans, lookups.taskModelById]);

  const planModelById = useMemo(() => {
    const m = new Map<string, unknown>();
    for (const p of filteredPlans) {
      m.set(p.id, {
        ...p,
        driftCount: driftByPlan.get(p.id)?.length ?? 0,
        workspaceScope: workspaceScopeByPlan.get(p.id),
        specTitleById: specTitleByPlan.get(p.id),
        sectionAgentsById: sectionAgentsByPlan.get(p.id),
      });
    }
    return m;
  }, [filteredPlans, driftByPlan, workspaceScopeByPlan, specTitleByPlan, sectionAgentsByPlan]);
  const specModelById = useMemo(() => {
    const m = new Map<string, unknown>();
    for (const p of filteredPlans) {
      const specSections = specSectionsByPlan.get(p.id);
      m.set(
        p.id,
        specSections
          ? { ...p, sections: specSections, tasks: [], workspaceScope: workspaceScopeByPlan.get(p.id) }
          : { ...p, workspaceScope: workspaceScopeByPlan.get(p.id) },
      );
    }
    return m;
  }, [filteredPlans, specSectionsByPlan, workspaceScopeByPlan]);
  const ghostModelById = useMemo(() => {
    const m = new Map<string, PlanGhostModel>();
    for (const [planId, meta] of lineageCatalog) {
      m.set(planId, { planId: meta.id, title: meta.title, status: meta.status, sectionCount: meta.sectionCount, taskCount: meta.taskCount });
    }
    return m;
  }, [lineageCatalog]);

  const resolveModel = useCallback(
    (node: LaidOutNode): unknown => {
      switch (node.kind) {
        case "plan":
          return planModelById.get(node.id);
        case "spec":
          return specModelById.get(node.planId);
        case "task":
          return lookups.taskModelById.get(node.id);
        case "plan-ghost": {
          const planId = node.id.startsWith("ghost:") ? node.id.slice(6) : node.id;
          return ghostModelById.get(planId) ?? { planId, title: planId, status: "archived" };
        }
        default:
          return lookups.elementById.get(node.id);
      }
    },
    [planModelById, specModelById, ghostModelById, lookups],
  );


  const [tagFilter, setTagFilter] = useState<TagFilterSelection>(() => new Map());
  const onToggleTagFilter = useCallback((tag: string) => {
    setTagFilter((prev) => toggleTagFilter(prev, tag));
  }, []);
  const tagFilterActive = isTagFilterActive(tagFilter);
  // When a tag filter is active, MATCHING task ids are kept lit; the canvas dims the rest.
  const tagFilterMatchIds = useMemo(() => {
    if (!tagFilterActive) return null;
    const ids = new Set<string>();
    for (const [taskId, model] of lookups.taskModelById) {
      if (taskMatchesTagFilter(model.task.tags, tagFilter)) ids.add(taskId);
    }
    return ids;
  }, [tagFilter, tagFilterActive, lookups.taskModelById]);

  useCanvasFrameProbe(transform);

  const padding = viewportPadding ?? DEFAULT_CANVAS_SETTINGS.cullPadding;
  const canvasSize = useMemo(() => ({ width: size.width, height: size.height }), [size.width, size.height]);
  const nodeIds = useStableSet(
    useMemo(
      () => new Set(visibleNodeIds(layout, transform, canvasSize, padding)),
      [layout, transform, canvasSize, padding],
    ),
  );

  const bandRef = useRef<Map<string, World>>(new Map());
  const worldByNodeRef = useRef<Map<string, World>>(new Map());
  const worldByNode = useMemo(() => {
    const prev = bandRef.current;
    const next = new Map<string, World>();
    for (const frame of layout.frames) {
      for (const n of frame.nodes) {
        if (!nodeIds.has(n.id) && n.kind !== "plan") continue;
        const t = worldThresholds(n.kind);
        next.set(n.id, nextWorld(prev.get(n.id), n.box.width * transform.scale, t));
      }
    }
    bandRef.current = next;
    const prevOut = worldByNodeRef.current;
    if (sameWorldMap(prevOut, next)) return prevOut;
    worldByNodeRef.current = next;
    return next;
  }, [layout, transform.scale, nodeIds]);

  const [focusedNodeId, setFocusedNodeId] = useState<string | null>(null);
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  // OVERLAYS the map without moving the camera, so closing returns to the exact spot.
  const [detailNodeId, setDetailNodeId] = useState<string | null>(null);
  const nav = useCanvasNavHistory();

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
  } = useCanvasFlyTo({ lookups, planById, size, fitToView, animateToView, animateTo, setTransform, recordNav: nav.push });

  const { presenceOverlays, swarmRoster } = useCanvasOverlays({
    scene,
    transform,
    focusedNodeId,
    mode,
    planId,
    lookups,
    nodeIds,
    worldByNode,
    staleActiveTaskIds,
  });

  // §25.3 dominance: a task filling ≳60% of the viewport elevates (emphasis ring).
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

  const presenceEmphasisIds = useMemo(() => {
    const ids = new Set<string>();
    if (hoveredNodeId) ids.add(hoveredNodeId);
    if (detailNodeId) ids.add(detailNodeId);
    if (focusedNodeId) ids.add(focusedNodeId);
    if (dominantNodeId) ids.add(dominantNodeId);
    for (const entry of activeWork.entries) ids.add(entry.task.id);
    return ids;
  }, [hoveredNodeId, detailNodeId, focusedNodeId, dominantNodeId, activeWork.entries]);

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
    () => (filteredPlans.find((p) => p.owner.tab_id) ?? filteredPlans[0])?.id ?? null,
    [filteredPlans],
  );

  const tabTaskIds = useMemo(
    () => (focusTabId ? assignedTaskIdsForTab(filteredPlans, focusTabId) : []),
    [focusTabId, filteredPlans],
  );

  const workspaceFocusIds = useMemo(
    () => resolveWorkspaceFocusNodeIds(mode, workspaceTarget, filteredPlans),
    [mode, workspaceTarget, filteredPlans],
  );

  const fittedRef = useRef<string | null>(null);
  const fitContextKey = canvasFitContextKey(mode, focusTabId, workspaceTarget);
  useEffect(() => {
    if (size.width <= 0 || size.height <= 0 || layout.bounds.width <= 0) return;
    if (fittedRef.current === fitContextKey) return;
    fittedRef.current = fitContextKey;
    // frames the active plan (§23.4), not fit-all (which lands at 5% on a big board).
    if (workspaceFocusIds.length) focusOnNodes(workspaceFocusIds);
    else if (tabTaskIds.length) focusOnNodes(tabTaskIds);
    else if (activePlanId) {
      const frame = layout.frames.find((f) => f.planId === activePlanId);
      if (frame) fitToView(frame.box);
      else fitToView(layout.bounds);
    } else fitToView(layout.bounds);
  }, [fitContextKey, fitToView, fitCameraToNodes, focusOnNodes, workspaceFocusIds, tabTaskIds, activePlanId, size.width, size.height]);

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

  // camera), and the zone name labels. Camera-decoupled — rebuilt only on layout change.
  const allNodes = useMemo(() => layout.frames.flatMap((f) => f.nodes), [layout]);
  const allEdges = useMemo(() => [...layout.frames.flatMap((f) => f.edges), ...(layout.crossEdges ?? [])], [layout]);
  const zoneLabels = useMemo(() => {
    const out: { text: string; anchor: { x: number; y: number } }[] = [];
    for (const frame of layout.frames) {
      for (const z of frame.zones) {
        const text = sectionTitleById.get(z.sectionId) ?? z.label;
        if (text) out.push({ text, anchor: z.labelAnchor });
      }
    }
    return out;
  }, [layout, sectionTitleById]);

  const sectionZones = useMemo(() => {
    const out: { hull: { x: number; y: number }[]; color: string; box?: { x: number; y: number; width: number; height: number } }[] = [];
    for (const frame of layout.frames) {
      for (const z of frame.zones) {
        out.push({ hull: z.hull, color: z.color, box: z.box });
      }
    }
    return out;
  }, [layout]);

  // Orchestrator avatars + their plan connectors (drawn on the canvas, §32.5).
  const groupGlyphs = useMemo<GroupGlyph[]>(() => {
    const out: GroupGlyph[] = [];
    for (const group of layout.groups) {
      const planAnchors: { x: number; y: number }[] = [];
      for (const f of layout.frames) {
        if (!group.planIds.includes(f.planId)) continue;
        const pn = f.nodes.find((n) => n.kind === "plan");
        if (pn) planAnchors.push({ x: pn.box.x + pn.box.width / 2, y: pn.box.y });
      }
      out.push({ ownerKey: group.ownerKey, label: group.title, live: !!group.ownerTabId, anchor: { x: group.box.x + 24, y: group.box.y + 34 }, planAnchors });
    }
    return out;
  }, [layout]);

  // taskId → live agents (on-card presence swarm), camera-decoupled.
  const presenceByNode = useMemo(() => {
    const m = new Map<string, PresenceAgent[]>();
    for (const [id, model] of lookups.taskModelById) {
      const agents = model.presence?.agents;
      if (agents && agents.length) m.set(id, agents);
    }
    return m;
  }, [lookups.taskModelById]);

  const presenceBySection = useMemo(() => {
    const out = new Map<string, PresenceAgent[]>();
    for (const [planId, bySection] of sectionAgentsByPlan) {
      for (const [sectionId, agents] of bySection) out.set(`${planId}:${sectionId}`, agents);
    }
    return out;
  }, [sectionAgentsByPlan]);

  const lodGroups = useMemo(
    () =>
      buildSectionGroups(
        allNodes,
        lookups.taskById,
        (_planId, sectionId) => sectionTitleById.get(sectionId) ?? (sectionId === "__unsectioned__" ? "Tasks" : sectionId),
        presenceBySection,
      ),
    [allNodes, lookups.taskById, sectionTitleById, presenceBySection],
  );

  const sceneLod = useMemo(
    () => resolveSceneLod(transform.scale, lodGroups, lookups.taskById),
    [transform.scale, lodGroups, lookups.taskById],
  );

  // Fly-arrival blink + off-screen-change plan pulse → one emphasis-ring set on the canvas.
  const blinkSet = useMemo(() => new Set<string>([...blinkIds, ...pulsePlanIds]), [blinkIds, pulsePlanIds]);
  const highlightIds = highlight?.taskIds ?? null;

  const fitView = useCallback(() => fitToView(layout.bounds), [fitToView, layout.bounds]);
  const resetView = useCallback(() => {
    const cx = layout.bounds.x + layout.bounds.width / 2;
    const cy = layout.bounds.y + layout.bounds.height / 2;
    setTransform({ scale: 1, x: size.width / 2 - cx, y: size.height / 2 - cy });
  }, [layout.bounds, size.width, size.height, setTransform]);

  // §23.4 camera presets + focus gesture — camera-only (no node modes, §24.1).
  // Content reveals itself by zoom: Overview = zoom out (compact), Read = zoom in.
  const overviewPreset = useCallback(() => {
    setFocusedNodeId(null);
    nav.clear();
    fitToView(layout.bounds);
  }, [fitToView, layout.bounds, nav]);
  const readPreset = useCallback(() => {
    if (activePlanId) {
      setFocusedNodeId(activePlanId);
      nav.clear();
      fitCameraToNodes([activePlanId]);
    }
  }, [activePlanId, fitCameraToNodes, nav]);
  const focusNodeCamera = useCallback(
    (id: string) => {
      setFocusedNodeId(id);
      fitCameraToNodes([id]);
      if (id !== activePlanId) nav.push({ id, kind: planById.has(id) ? "plan" : "task", label: planById.get(id)?.title ?? lookups.taskById.get(id)?.name ?? id, flyIds: [id] });
    },
    [fitCameraToNodes, activePlanId, nav, planById, lookups],
  );

  // RETURN-TO-PLAN: the plan is home base. Home frames the plan + clears the path;
  // Back pops one hop and flies to the new top. Drives Esc-to-return (no breadcrumb UI).
  const flyToEntry = useCallback(
    (entry: NavEntry) => {
      const ids = entry.flyIds.filter((id) => lookups.nodeById.has(id));
      if (ids.length) focusOnNodes(ids, undefined, true);
    },
    [lookups, focusOnNodes],
  );
  const goHome = useCallback(() => {
    nav.clear();
    setFocusedNodeId(null);
    if (activePlanId && lookups.nodeById.has(activePlanId)) focusOnNodes([activePlanId], undefined, true);
    else fitToView(layout.bounds);
  }, [nav, activePlanId, lookups, focusOnNodes, fitToView, layout.bounds]);
  const goBack = useCallback(() => {
    const next = nav.path[nav.path.length - 2];
    nav.back();
    if (next) flyToEntry(next);
    else goHome();
  }, [nav, flyToEntry, goHome]);

  useEffect(() => {
    if (!nav.path.length && !focusedNodeId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (nav.path.length) goBack();
      else setFocusedNodeId(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [nav.path.length, focusedNodeId, goBack]);

  const onWorldDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      const rect = containerRef.current?.getBoundingClientRect();
      const hit = canvasApiRef.current?.hitTest({ x: e.clientX - (rect?.left ?? 0), y: e.clientY - (rect?.top ?? 0) });
      if (hit?.action.type === "section-cluster" || hit?.action.type === "done-stack") {
        onSectionFrame({ planId: hit.action.planId, sectionId: hit.action.sectionId, variant: "plan" });
        return;
      }
      if (hit) focusNodeCamera(hit.node.id);
    },
    [containerRef, focusNodeCamera, onSectionFrame],
  );

  const onCanvasClick = useCallback(
    (e: React.MouseEvent) => {
      const rect = containerRef.current?.getBoundingClientRect();
      const point = { x: e.clientX - (rect?.left ?? 0), y: e.clientY - (rect?.top ?? 0) };
      const hit = canvasApiRef.current?.hitTest(point);
      if (hit) {
        const { node, action } = hit;
        if (action.type === "expand") { setDetailNodeId(node.id); return; }
        if (action.type === "section-button" || action.type === "section") {
          onSectionFrame({ planId: node.planId, sectionId: action.sectionId, variant: node.kind === "spec" ? "spec" : "plan" });
          return;
        }
        if (action.type === "section-cluster" || action.type === "done-stack") {
          onSectionFrame({ planId: action.planId, sectionId: action.sectionId, variant: "plan" });
          return;
        }
        if (action.type === "node") { focusNodeCamera(node.id); return; }
      }
      onCanvasFlyClick(e);
    },
    [containerRef, onSectionFrame, focusNodeCamera, onCanvasFlyClick],
  );

  // canvas paints the ring as a primitive — no per-frame DOM). Skipped while panning.
  const onCanvasPointerMove = useCallback(
    (e: React.MouseEvent) => {
      if (isDragging || hoverRafRef.current != null) return;
      const cx = e.clientX;
      const cy = e.clientY;
      hoverRafRef.current = requestAnimationFrame(() => {
        hoverRafRef.current = null;
        const rect = containerRef.current?.getBoundingClientRect();
        const hit = canvasApiRef.current?.hitTest({ x: cx - (rect?.left ?? 0), y: cy - (rect?.top ?? 0) });
        const nodeId = hit?.node.id ?? null;
        canvasApiRef.current?.setHover(nodeId);
        setHoveredNodeId(nodeId);
      });
    },
    [isDragging, containerRef],
  );


  const showsData = planFetchShowsData(loading, error ?? null, plans.length);

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
        onClick={onCanvasClick}
        onMouseMove={onCanvasPointerMove}
        onDoubleClick={onWorldDoubleClick}
        style={{
          cursor: isDragging ? "grabbing" : "default",
          userSelect: "none",
          touchAction: "none",
          background: "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
        }}
      >
        <PlanFetchReconnectingBanner
          reconnecting={reconnecting}
          itemCount={plans.length}
          testId="canvas-reconnecting"
          className="absolute top-2 left-3 z-30"
        />
        <div className="absolute inset-0 flex items-center justify-center">
          <PlanFetchErrorState
            loading={loading}
            error={error}
            itemCount={plans.length}
            onRetry={onRetry}
            testId="canvas-fetch"
          />
        </div>
        {showsData && filteredPlans.length === 0 && (
          <div
            data-testid="canvas-empty"
            className="absolute inset-0 flex flex-col items-center justify-center text-sm px-6 text-center gap-4"
            style={{ color: "var(--ct-fg)", opacity: "var(--ct-text-secondary-opacity)" }}
          >
            <p>No active plans yet.</p>
            {mode === "workspace" && teamId && api && <CanvasEmptyActions api={api} projectPath={projectPath} />}
          </div>
        )}
        {!loading && filteredPlans.length > 0 && (
          <div
            ref={layerRef}
            data-testid="canvas-world-layer"
            className="pointer-events-none absolute inset-0"
            style={{ transformOrigin: "0 0", zIndex: 10, overflow: "visible" }}
          >
            <CanvasPresenceLayer overlays={presenceOverlays} emphasisTaskIds={presenceEmphasisIds} onFlyToTask={onFlyToTask} />
          </div>
        )}
        {showsData && filteredPlans.length > 0 && (
          <CanvasSwarmCluster roster={swarmRoster} onFlyToTask={onFlyToTask} reserveTopRight={!!onClose} />
        )}
        {!loading && filteredPlans.length > 0 && (
          <CanvasWorldCanvas
            width={size.width}
            height={size.height}
            transform={transform}
            transformRef={transformRef}
            drawRequestRef={drawRequestRef}
            apiRef={canvasApiRef}
            nodes={allNodes}
            nodeById={lookups.nodeById}
            edges={allEdges}
            zoneLabels={zoneLabels}
            sectionZones={sectionZones}
            groups={groupGlyphs}
            presenceByNode={presenceByNode}
            resolveModel={resolveModel}
            revision={`${layoutRevision}\0${plansRevision}`}
            dimIds={tagFilterMatchIds}
            highlightIds={highlightIds}
            blinkIds={blinkSet}
            dominantId={dominantNodeId}
            lodGroups={lodGroups}
            taskById={lookups.taskById}
            onCameraPaint={paintWorldLayer}
          />
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
      {showsData && filteredPlans.length > 0 && (
        <FlyToActiveControl entries={activeWork.entries} onFly={onFlyToTask} />
      )}
      {showsData && filteredPlans.length > 0 && (
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
      {legendFacets.length > 0 && showsData && filteredPlans.length > 0 && (
        <CanvasTagLegend facets={legendFacets} filter={tagFilter} onToggleTag={onToggleTagFilter} />
      )}
      {minimapOn && showsData && filteredPlans.length > 0 && (
        <CanvasMinimap
          bounds={layout.bounds}
          nodes={[...lookups.nodeById.values()]}
          transform={transform}
          viewport={size}
          hiddenNodeIds={sceneLod.hiddenNodeIds}
          lodMarkers={sceneLod.markers}
        />
      )}
      <CanvasDetailOverlay
        nodeId={detailNodeId}
        nodeById={lookups.nodeById}
        resolveModel={resolveModel}
        tabs={tabs}
        api={api}
        onClose={() => setDetailNodeId(null)}
        onOpenTab={onOpenTab}
        onOpenTask={(taskId) => setDetailNodeId(taskId)}
        onCopyLink={onCopyLink}
        onFocusStride={onFocusStride}
        onSectionClick={onSectionClick}
        onSectionFrame={onSectionFrame}
      />
    </div>
  );
}

const EMPTY_PLANS: Plan[] = [];

/** Owner tab ids of the fixture plans actually handed to the shell. */
export function fixtureOwnerTabIds(fixturePlans: readonly Plan[], shownPlans: readonly Plan[]): string[] {
  const shown = new Set(shownPlans.map((p) => p.id));
  const ids = new Set<string>();
  for (const plan of fixturePlans) {
    if (!shown.has(plan.id)) continue;
    const tabId = plan.owner.tab_id?.trim();
    if (tabId) ids.add(tabId);
  }
  return [...ids];
}

/** A fixture world is synthetic: its owner tab can never appear in a real
 *  workspace, so scoping would drop every fixture plan. Bind those owners to the
 *  scoped workspace instead of disabling scoping, which still rejects plans from
 *  other workspaces. */
export function bindFixtureOwnersToWorkspace(
  workspaces: Workspace[],
  workspaceId: string | undefined,
  ownerTabIds: readonly string[],
): Workspace[] {
  if (!workspaceId || ownerTabIds.length === 0) return workspaces;
  return workspaces.map((ws) => {
    if (ws.id !== workspaceId) return ws;
    const missing = ownerTabIds.filter((id) => !ws.tabs.includes(id));
    return missing.length ? { ...ws, tabs: [...ws.tabs, ...missing] } : ws;
  });
}

export interface CanvasViewProps {
  mode: "workspace" | "focus";
  workspaceId?: string;
  planId?: string;
  orchestratorTabId?: string;
  focusTabId?: string | null;
  api: ApiConfig;
  onClose?: () => void;
}

export function CanvasView({ mode, workspaceId, planId, orchestratorTabId, focusTabId, api, onClose }: CanvasViewProps) {
  const { setFocusedTab, setActiveTab, selectTabByTab } = useMultiplexer();
  const state = useMuxState();
  const openPlanTarget = usePlanTargetNavigation();
  const onOpenTab = useCallback(
    (tabId: string) => {
      revealAssignedAgentTab(
        { workspaces: state.workspaces, setActiveTab, setFocusedTab },
        tabId,
      );
    },
    [state.workspaces, setActiveTab, setFocusedTab],
  );
  const activeWs = state.workspaces.find((ws) => ws.id === state.active_workspace_id);
  const projectPath = activeWs?.cwd ?? null;
  const workspaceTarget = useCanvasWorkspaceTarget();
  const requestedPlanId = planId ?? (mode === "workspace" ? workspaceTarget?.planId : undefined);
  const teamId = mode === "workspace" ? workspaceTarget?.teamId : undefined;
  const { plans: livePlans, loading, reconnecting, error, refetch } = useCanvasPlans(
    mode,
    projectPath,
  );
  // Dev harness: real exported plan, or a synthetic N-task plan, when flagged.
  const perfN = perfFixtureCount();
  const perfProfile = perfFixtureProfile();
  const useReal = realFixtureEnabled();
  const fixtureActive = useReal || perfProfile !== "default" || perfN > 0;
  const fixturePlans = useMemo(() => {
    if (!fixtureActive) return EMPTY_PLANS;
    return useReal
        ? realFixturePlans()
        : perfProfile === "card-metadata"
          ? buildCardMetadataFixturePlans()
          : perfProfile === "plan-with-known-tasks"
            ? buildPlanWithKnownTasksFixturePlans()
            : perfProfile === "tab-tree-with-statuses"
              ? buildTabTreeWithStatusesFixturePlans()
              : perfProfile === "mid-stream-chat-transcript"
                ? buildMidStreamChatTranscriptFixturePlans()
          : perfProfile === "done-heavy"
            ? buildDoneHeavyPerfFixturePlans()
            : buildPerfFixturePlans(perfN);
  }, [fixtureActive, useReal, perfProfile, perfN]);
  const plans = useMemo(
    () => (fixtureActive ? selectCanvasPlans(livePlans, fixturePlans, requestedPlanId) : livePlans),
    [fixtureActive, fixturePlans, livePlans, requestedPlanId],
  );
  // (the live-tab gate filters real plans correctly; this only feeds the demo plan).
  const tabs = useMemo(() => {
    if (perfN <= 0 || useReal) return state.tabs;
    const fixtureTabs =
      perfProfile === "card-metadata"
        ? buildCardMetadataFixtureTabs()
        : perfProfile === "tab-tree-with-statuses"
          ? buildTabTreeWithStatusesFixtureTabs()
          : perfProfile === "mid-stream-chat-transcript"
            ? buildMidStreamChatTranscriptFixtureTabs()
            : buildPerfFixtureTabs(plans);
    return { ...state.tabs, ...fixtureTabs };
  }, [perfN, useReal, perfProfile, plans, state.tabs]);
  const scopeWorkspaceId =
    mode === "workspace" ? workspaceId ?? state.active_workspace_id ?? undefined : undefined;
  const shownFixtureOwnerTabIds = useMemo(
    () => fixtureOwnerTabIds(fixturePlans, plans),
    [fixturePlans, plans],
  );
  const scopedWorkspaces = useMemo(
    () => bindFixtureOwnersToWorkspace(state.workspaces, scopeWorkspaceId, shownFixtureOwnerTabIds),
    [state.workspaces, scopeWorkspaceId, shownFixtureOwnerTabIds],
  );
  return (
    <PlanViewShell
      mode={mode}
      plans={plans}
      loading={loading}
      reconnecting={reconnecting}
      error={error}
      onRetry={refetch}
      planId={requestedPlanId}
      teamId={teamId}
      orchestratorTabId={orchestratorTabId}
      focusTabId={focusTabId}
      tabs={tabs}
      workspaces={scopedWorkspaces}
      workspaceId={scopeWorkspaceId}
      teams={state.teams}
      projectPath={projectPath}
      workspaceTarget={mode === "workspace" ? workspaceTarget : null}
      onClose={onClose}
      onOpenTab={onOpenTab}
      notifications={state.notifications}
      onSelectTab={selectTabByTab}
      onOpenPlanTarget={openPlanTarget}
      api={api}
    />
  );
}
