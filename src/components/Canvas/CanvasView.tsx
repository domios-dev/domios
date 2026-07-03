import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { ApiConfig, Pane, Workspace } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { resolveCanvasFocusNodeIds, type CanvasFocusPayload } from "./canvasFocusPayload";
import { CanvasEmptyActions } from "./CanvasEmptyActions";
import { CanvasToolbar } from "./CanvasToolbar";
import { CanvasMinimap } from "./CanvasMinimap";
import { CanvasTagLegend } from "./CanvasTagLegend";
import { CanvasUnownedTray } from "./CanvasUnownedTray";
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
import { assignedTaskIdsForPane } from "./planOpenFocus";
import { filterPlansForCanvas } from "./planPaneMatch";
import { useCanvasPlans } from "./PlansProvider";
import { useCanvasWorkspaceTarget } from "./canvasFocusStore";
import { isTagFilterActive, taskMatchesTagFilter, toggleTagFilter, type TagFilterSelection } from "./tagModel";
import type { PlanGhostModel } from "./canvasView/models";
import type { OrchestratorRef, Plan, Section, TagFacet } from "./types";
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
import { perfFixtureCount, perfFixtureProfile, buildPerfFixturePlans, buildDoneHeavyPerfFixturePlans, buildPerfFixturePanes, realFixtureEnabled, realFixturePlans, fixtureSpecSectionsByPlan } from "./perfFixture";
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

export type { TaskCardModel } from "./canvasView/models";

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

function sameWorldMap(a: Map<string, World>, b: Map<string, World>): boolean {
  if (a.size !== b.size) return false;
  for (const [k, v] of b) if (a.get(k) !== v) return false;
  return true;
}

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
  useEffect(() => {
    bumpCanvasChurn("canvasCommits");
  });

  // Computed every render (value-based change detector) so an in-place plan-body
  // mutation is still caught — downstream memos key on the STRING value, so they
  // only recompute when content actually changes. (The heavier layout fingerprint is
  // memoized below; it chains off this via filteredPlans, so in-place still propagates.)
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
  // Region labels resolve from the plan's OWN sections (sec_… ids + titles). The layout
  // plan gets its sections OVERLAID with spec sections (plansForLayout), so its ids no
  // longer match the tasks' section_ids — resolve titles from the original plans here.
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

  // The <canvas> renderer publishes its blit-scheduler here; the viewport calls it
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

  // §24.1: ONE behavior — no manual view modes. Node frames are fixed; content
  // adapts to camera zoom (content-LOD). The toolbar is camera-only.
  const [minimapOn, setMinimapOn] = useState(true);

  // Memoize the layout source + its fingerprint: plansForLayout maps a NEW array when
  // spec sections are present, so an unmemoized call minted a fresh ref every render →
  // the scene memo (dep: layoutSourcePlans) rebuilt every render. Stable now.
  const layoutSourcePlans = useMemo(() => plansForLayout(filteredPlans, specSectionsByPlan), [filteredPlans, specSectionsByPlan]);
  const layoutRevision = useMemo(() => layoutInputRevision(layoutSourcePlans, specSectionsByPlan), [layoutSourcePlans, specSectionsByPlan]);
  const lineageCatalog = useMemo(() => buildLineageCatalog(plans), [plansRevision]);
  const layout = useMemo(() => layoutWorkspace(layoutSourcePlans, undefined, sectionTitleById), [layoutRevision, sectionTitleById]);
  // Scale-independent: scene structure does not depend on camera zoom, so it
  // rebuilds only when the layout changes — not on every pan/zoom frame.
  const scene = useMemo(() => {
    bumpCanvasChurn("sceneBuilds");
    return buildCanvasScene({ layout, plans: layoutSourcePlans });
  }, [layout, layoutSourcePlans]);

  // #8: a pane heartbeat mutates the whole `panes` map ref, but buildTaskPresence
  // reads only EXISTENCE + title of the panes referenced by tasks (assigned/lane +
  // milestone authors). Key on exactly that so a heartbeat that changes nothing
  // presence-relevant does NOT rebuild all ~N task models.
  const panesContentKey = useMemo(() => buildTaskPresenceFingerprint(filteredPlans, panes), [filteredPlans, panes]);
  const lookups = useCanvasLookups(layout, filteredPlans, driftByPlan, panes, panesContentKey);

  // Per plan: sectionId → LIVE agents working it (task→section roll-up). Live-only,
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

  // #4: pre-mint ONE stable model object per plan/spec/ghost, rebuilt only on a data
  // change — resolveModel returns the cached identity so CanvasNodeCard's memo holds
  // across pan/zoom (the old per-call spread minted a fresh object every render).
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
  // Build the visible-id set, then collapse identity when membership is unchanged
  // (#2 cascade root): a pan that reveals no new node keeps the SAME Set reference,
  // so every memo'd child below skips its re-render instead of cascading.
  const nodeIds = useStableSet(
    useMemo(
      () => new Set(visibleNodeIds(layout, transform, canvasSize, padding)),
      [layout, transform, canvasSize, padding],
    ),
  );

  // §26 worlds: each node's content band is driven by its OWN appW (footprint ×
  // camera scale), not the global scale. Hysteresis (prev band, ±10%) kills flicker.
  // #1: only VISIBLE nodes (+ every plan node — the zone-label site needs plan-level
  // bands even when the plan node is off-screen) are banded; and the returned Map keeps
  // its identity across scale ticks while no band actually crosses, so memo'd consumers
  // skip re-render during a zoom that doesn't change any node's band.
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
  // UX-2 READ surface: the node whose fullscreen detail is open (null = none). It
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
    return ids;
  }, [hoveredNodeId, detailNodeId, focusedNodeId, dominantNodeId]);

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
    else if (activePlanId) {
      const frame = layout.frames.find((f) => f.planId === activePlanId);
      if (frame) fitToView(frame.box);
      else fitToView(layout.bounds);
    } else fitToView(layout.bounds);
  }, [fitContextKey, fitToView, fitCameraToNodes, focusOnNodes, workspaceFocusIds, paneTaskIds, activePlanId, size.width, size.height]);

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

  // Canvas-renderer inputs: ALL laid-out nodes/edges (the draw loop culls live to the
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
      out.push({ ownerKey: group.ownerKey, label: group.title, live: !!group.ownerPaneId, anchor: { x: group.box.x + 24, y: group.box.y + 34 }, planAnchors });
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

  // The world is a <canvas> now — no per-node DOM to hit-test. Double-click uses the
  // SAME smallest-area hit-test as single-click (canvasApiRef.hitTest), so a click and a
  // double-click at one point always resolve to the same node — then focuses it.
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

  // Single-click → canvas hit-test → action: ⤢ opens the full detail overlay; a
  // section / "→ tasks" button navigates to that section's tasks; node body focuses.
  // Falls through to the delegated fly-click (rich-overlay chips, swarm rows).
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

  // Hover → highlight the node/region under the pointer (rAF-throttled hit-test; the
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
            ref={layerRef}
            data-testid="canvas-world-layer"
            className="pointer-events-none absolute inset-0"
            style={{ transformOrigin: "0 0", zIndex: 10, overflow: "visible" }}
          >
            <CanvasPresenceLayer overlays={presenceOverlays} emphasisTaskIds={presenceEmphasisIds} onFlyToTask={onFlyToTask} />
          </div>
        )}
        {!loading && !error && filteredPlans.length > 0 && (
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
      {legendFacets.length > 0 && !loading && !error && filteredPlans.length > 0 && (
        <CanvasTagLegend facets={legendFacets} filter={tagFilter} onToggleTag={onToggleTagFilter} />
      )}
      {minimapOn && !loading && !error && filteredPlans.length > 0 && (
        <CanvasMinimap
          bounds={layout.bounds}
          nodes={[...lookups.nodeById.values()]}
          transform={transform}
          viewport={size}
          hiddenNodeIds={sceneLod.hiddenNodeIds}
          lodMarkers={sceneLod.markers}
        />
      )}
      {api && mode === "workspace" && <CanvasUnownedTray api={api} claimOwner={claimOwner} />}
      <CanvasDetailOverlay
        nodeId={detailNodeId}
        nodeById={lookups.nodeById}
        resolveModel={resolveModel}
        panes={panes}
        onClose={() => setDetailNodeId(null)}
        onOpenPane={onOpenPane}
        onCopyLink={onCopyLink}
        onFocusStride={onFocusStride}
        onSectionClick={onSectionClick}
        onSectionFrame={onSectionFrame}
      />
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
  const perfProfile = perfFixtureProfile();
  const useReal = realFixtureEnabled();
  const plans = useMemo(
    () =>
      useReal
        ? realFixturePlans()
        : perfProfile === "done-heavy"
          ? buildDoneHeavyPerfFixturePlans()
          : perfN > 0
            ? buildPerfFixturePlans(perfN)
            : livePlans,
    [useReal, perfProfile, perfN, livePlans],
  );
  // Fixture-only: synthesize live panes for the synthetic swarm so presence renders
  // (the live-pane gate filters real plans correctly; this only feeds the demo plan).
  const panes = useMemo(
    () => (perfN > 0 && !useReal ? { ...state.panes, ...buildPerfFixturePanes(plans) } : state.panes),
    [perfN, useReal, plans, state.panes],
  );
  return (
    <PlanViewShell
      mode={mode}
      plans={plans}
      loading={loading}
      error={error}
      planId={planId}
      orchestratorPaneId={orchestratorPaneId}
      focusPaneId={focusPaneId}
      panes={panes}
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
