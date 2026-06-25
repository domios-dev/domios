import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { listen } from "../../lib/bridge";
import type { ApiConfig, Pane } from "../../lib/types";
import { useMultiplexer } from "../../store/multiplexer";
import { CanvasComposer } from "./CanvasComposer";
import { resolveCanvasFocusNodeIds, type CanvasFocusPayload } from "./canvasFocusPayload";
import { CanvasEmptyActions } from "./CanvasEmptyActions";
import { CanvasToolbar } from "./CanvasToolbar";
import { CanvasMinimap } from "./CanvasMinimap";
import { CanvasTagLegend } from "./CanvasTagLegend";
import { CanvasUnownedTray } from "./CanvasUnownedTray";
import type { CanvasLod, MilestoneBubbleModel } from "./cards";
import { OrchestratorGroup } from "./cards";
import { getNodeRenderer, type SectionNavArgs } from "./nodeRegistry";
import { bezierPath, resolveEdgeEnds } from "./edges";
import { milestoneColorMap } from "./milestoneColors";
import { sectionRows, type SectionRowGlyph } from "./sectionRows";
import {
  layoutWorkspace,
  layoutInputRevision,
  plansForLayout,
  visibleFrameIds,
  visibleNodeIds,
  viewportWorldRect,
  boxesOverlap,
  type LaidOutNode,
} from "./layout";
import { archivePlan, fetchSpecSections } from "./plansApi";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";
import { crispRasterScale, nextWorld, worldThresholds, type World } from "./worlds";
import { assignedTaskIdsForPane } from "./planOpenFocus";
import { filterPlansForCanvas } from "./planPaneMatch";
import { useCanvasPlans } from "./PlansProvider";
import { isTagFilterActive, taskMatchesTagFilter, toggleTagFilter, type TagFilterSelection } from "./tagModel";
import type { CanvasElement, DriftItem, OrchestratorRef, Plan, Section, TagFacet, Task } from "./types";
import { usePlanDrift } from "./usePlanDrift";
import { useCanvasViewport, formatLayerTransform } from "./useCanvasViewport";

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
  focusedPaneId?: string | null;
  projectPath?: string | null;
  onClose?: () => void;
  onOpenPane?: (paneId: string) => void;
  /** Talk-to-area composer needs an API config to POST to the owner pane. */
  api?: ApiConfig;
  /** Test hook: expand viewport culling margin. */
  viewportPadding?: number;
}

// Deterministic [0,1) hash of an edge id → stable per-edge curvature jitter.
function edgeJitter(id: string): number {
  let h = 2166136261;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return ((h >>> 0) % 1000) / 1000;
}

function lodForScale(scale: number): CanvasLod {
  const s = DEFAULT_CANVAS_SETTINGS;
  if (scale < s.zoomFarThreshold) return "far";
  if (scale < s.zoomNearThreshold) return "mid";
  return "near";
}

// Fixed bounds (§22.1): height is the layout-reserved box height, NOT minHeight —
// content must fit/scroll inside, never push the node past its slot (overflow bug).
function nodeStyle(box: { x: number; y: number; width: number; height?: number }) {
  return {
    position: "absolute" as const,
    left: box.x,
    top: box.y,
    width: box.width,
    height: box.height,
  };
}

export interface ClusterCaptionModel {
  kinds: { label: string; color: string }[];
}

export interface SpecSectionModel {
  section: Section;
  glyph: SectionRowGlyph;
  taskCount: number;
}

export interface TaskCardModel {
  task: Task;
  tagFacets?: TagFacet[];
  childCount: number;
  childDone: number;
  parentTaskName: string | null;
  stepColors: Map<string, string>;
  drift?: DriftItem;
}

interface CanvasLookups {
  nodeById: Map<string, LaidOutNode>;
  taskById: Map<string, Task>;
  taskModelById: Map<string, TaskCardModel>;
  sectionById: Map<string, Section>;
  specSectionById: Map<string, SpecSectionModel>;
  milestoneModelById: Map<string, MilestoneBubbleModel>;
  milestoneTaskById: Map<string, string>;
  clusterModelById: Map<string, ClusterCaptionModel>;
  elementById: Map<string, CanvasElement>;
}

function collectSections(sections: Section[], out: Map<string, Section>): void {
  for (const s of sections) {
    out.set(s.id, s);
    if (s.subsections.length) collectSections(s.subsections, out);
  }
}

interface CanvasNodeCardProps {
  node: LaidOutNode;
  model: unknown;
  lod: CanvasLod;
  world: World;
  crispScale: number;
  elevated?: boolean;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
  onArchive?: () => void;
  onFitNode?: (nodeId: string) => void;
  onCopyLink?: (id: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  tagFilterDimmed?: boolean;
  tagFilterHighlighted?: boolean;
}

const CanvasNodeCard = memo(
  function CanvasNodeCard({ node, model, lod, world, crispScale, elevated, onOpenPane, onHoverTask, onArchive, onFitNode, onCopyLink, onFocusStride, onSectionClick, onSectionFrame, tagFilterDimmed, tagFilterHighlighted }: CanvasNodeCardProps) {
    const base = nodeStyle(node.box);
    // §32.3 nodes are SOLID at every zoom — status/hierarchy/focus deprioritize
    // SPATIALLY (position, depth), never by fade. Elevation is depth, not opacity.
    const style: typeof base & { zIndex?: number; transition?: string; filter?: string } =
      elevated
        ? { ...base, zIndex: 20, filter: "drop-shadow(0 18px 40px rgba(0,0,0,0.55))", transition: "filter var(--ct-duration-normal) var(--ct-ease-decel)" }
        : base;
    const rendered = getNodeRenderer(node.kind)({
      node,
      model,
      style,
      lod,
      world,
      crispScale,
      onOpenPane,
      onHoverTask,
      onArchive,
      onFitNode,
      onCopyLink,
      onCopyStrideLink: onCopyLink,
      onFocusStride,
      onSectionClick,
      onSectionFrame,
      tagFilterDimmed,
      tagFilterHighlighted,
    });
    return rendered ? <Fragment key={node.id}>{rendered}</Fragment> : null;
  },
  (prev, next) => {
    if (prev.node.id !== next.node.id) return false;
    if (prev.lod !== next.lod) return false;
    if (prev.world !== next.world) return false;
    if (prev.crispScale !== next.crispScale) return false;
    if (prev.elevated !== next.elevated) return false;
    if (prev.tagFilterDimmed !== next.tagFilterDimmed) return false;
    if (prev.tagFilterHighlighted !== next.tagFilterHighlighted) return false;
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
  focusPaneId = null,
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
  const driftByPlan = usePlanDrift(api, filteredPlanIds);

  // Fetch the REAL linked Spec entity per plan (Spec=WHAT ⊥ Plan=HOW). The spec
  // node renders THESE sections, not the plan's — they are distinct entities.
  const [specSectionsByPlan, setSpecSectionsByPlan] = useState<Map<string, Section[]>>(new Map());
  useEffect(() => {
    if (!api) return;
    let active = true;
    const targets = filteredPlans.filter((p) => p.primary_spec_id);
    if (!targets.length) return;
    void Promise.all(
      targets.map((p) =>
        fetchSpecSections(api, p.primary_spec_id as string)
          .then((sections) => [p.id, sections] as const)
          .catch(() => [p.id, [] as Section[]] as const),
      ),
    ).then((pairs) => {
      if (active) setSpecSectionsByPlan(new Map(pairs));
    });
    return () => {
      active = false;
    };
  }, [api, filteredPlans]);

  const { transform, handlers, fitToView, zoomBy, setTransform, isDragging, containerRef, layerRef } = useCanvasViewport({
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
  const layout = useMemo(
    () => layoutWorkspace(layoutSourcePlans),
    [layoutRevision],
  );

  const lookups = useMemo<CanvasLookups>(() => {
    const nodeById = new Map<string, LaidOutNode>();
    for (const frame of layout.frames) for (const n of frame.nodes) nodeById.set(n.id, n);
    const taskById = new Map<string, Task>();
    const taskModelById = new Map<string, TaskCardModel>();
    const sectionById = new Map<string, Section>();
    const specSectionById = new Map<string, SpecSectionModel>();
    const milestoneModelById = new Map<string, MilestoneBubbleModel>();
    const milestoneTaskById = new Map<string, string>();
    const clusterModelById = new Map<string, ClusterCaptionModel>();
    const elementById = new Map<string, CanvasElement>();
    for (const plan of filteredPlans) {
      collectSections(plan.sections, sectionById);
      for (const row of sectionRows(plan)) {
        specSectionById.set(`${plan.id}::spec::${row.section.id}`, {
          section: row.section,
          glyph: row.glyph,
          taskCount: row.taskCount,
        });
      }
      const childCounts = new Map<string, number>();
      const childDone = new Map<string, number>();
      const taskNameById = new Map(plan.tasks.map((t) => [t.id, t.name]));
      for (const t of plan.tasks) {
        if (t.parent_task_id) {
          childCounts.set(t.parent_task_id, (childCounts.get(t.parent_task_id) ?? 0) + 1);
          if (t.status === "done" || t.status === "cancelled") childDone.set(t.parent_task_id, (childDone.get(t.parent_task_id) ?? 0) + 1);
        }
      }
      const colors = milestoneColorMap(plan);
      const driftForPlan = driftByPlan.get(plan.id) ?? [];
      const driftByTask = new Map<string, DriftItem>();
      for (const item of driftForPlan) {
        const existing = driftByTask.get(item.task_id);
        if (!existing || (existing.not_mine && !item.not_mine)) driftByTask.set(item.task_id, item);
      }
      for (const task of plan.tasks) {
        taskById.set(task.id, task);
        const parentTaskName = task.parent_task_id ? taskNameById.get(task.parent_task_id) ?? null : null;
        taskModelById.set(task.id, {
          task,
          tagFacets: plan.tag_facets,
          childCount: childCounts.get(task.id) ?? 0,
          childDone: childDone.get(task.id) ?? 0,
          parentTaskName,
          stepColors: colors,
          drift: driftByTask.get(task.id),
        });
        const seen = new Map<string, string>();
        for (const m of task.milestones) {
          const color = colors.get(m.kind) ?? "var(--ct-border-focus)";
          milestoneModelById.set(m.id, { milestone: m, color });
          milestoneTaskById.set(m.id, task.id);
          if (!seen.has(m.kind)) seen.set(m.kind, color);
        }
        if (seen.size) {
          clusterModelById.set(`${task.id}::cluster`, {
            kinds: [...seen.entries()].map(([label, color]) => ({ label: label.replace(/_/g, " "), color })),
          });
        }
      }
      for (const el of plan.elements) elementById.set(el.id, el);
    }
    return { nodeById, taskById, taskModelById, sectionById, specSectionById, milestoneModelById, milestoneTaskById, clusterModelById, elementById };
  }, [layout, filteredPlans, driftByPlan]);

  const resolveModel = useCallback(
    (node: LaidOutNode): unknown => {
      switch (node.kind) {
        case "plan":
          return planById.has(node.id) ? { ...planById.get(node.id)!, driftCount: driftByPlan.get(node.id)?.length ?? 0 } : undefined;
        case "spec": {
          const plan = planById.get(node.planId);
          const specSections = specSectionsByPlan.get(node.planId);
          // Render the REAL spec's sections (no tasks → coverage is a plan concept).
          return plan && specSections ? { ...plan, sections: specSections, tasks: [] } : plan;
        }
        case "task":
          return lookups.taskModelById.get(node.id);
        default:
          return lookups.elementById.get(node.id);
      }
    },
    [planById, lookups, specSectionsByPlan],
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

  const [blinkIds, setBlinkIds] = useState<Set<string>>(() => new Set());
  // Camera-focus reading gesture (§23.4): frames + locks the Document and shows a
  // breadcrumb; never fades neighbours (§32.3). Esc/back clears it. null = no focus.
  const [focusedNodeId, setFocusedNodeId] = useState<string | null>(null);

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

  // §31 change signal: a plan whose updated_at advanced changed live. The camera
  // does NOT move — in-viewport plans pulse gently, off-screen ones surface a
  // directional edge marker (click = user-initiated fly-there). Auto-clears.
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

  // Move the camera to frame a set of node ids (no blink) — the shared primitive.
  const fitCameraToNodes = useCallback(
    (ids: string[]) => {
      const boxes = ids.map((id) => lookups.nodeById.get(id)?.box).filter(Boolean) as {
        x: number; y: number; width: number; height: number;
      }[];
      if (!boxes.length) return;
      const minX = Math.min(...boxes.map((b) => b.x));
      const minY = Math.min(...boxes.map((b) => b.y));
      const maxX = Math.max(...boxes.map((b) => b.x + b.width));
      const maxY = Math.max(...boxes.map((b) => b.y + b.height));
      fitToView({ x: minX, y: minY, width: maxX - minX, height: maxY - minY });
    },
    [lookups, fitToView],
  );

  // Center the camera on a node set at an EXPLICIT scale (vs fit-to-bbox). The
  // product "go to node id at zoom Z" primitive: deterministic landing point.
  const centerCameraOnNodes = useCallback(
    (ids: string[], scale: number) => {
      const boxes = ids.map((id) => lookups.nodeById.get(id)?.box).filter(Boolean) as {
        x: number; y: number; width: number; height: number;
      }[];
      if (!boxes.length) return;
      const minX = Math.min(...boxes.map((b) => b.x));
      const minY = Math.min(...boxes.map((b) => b.y));
      const maxX = Math.max(...boxes.map((b) => b.x + b.width));
      const maxY = Math.max(...boxes.map((b) => b.y + b.height));
      const cx = (minX + maxX) / 2;
      const cy = (minY + maxY) / 2;
      const s = Math.max(0.05, Math.min(10, scale));
      setTransform({ x: size.width / 2 - cx * s, y: size.height / 2 - cy * s, scale: s });
    },
    [lookups, setTransform, size.width, size.height],
  );

  // Camera + blink onto a set of node ids (shared by canvas-focus + Plan-on-pane).
  // scale given → land centered at that exact zoom; omitted → fit the bbox.
  const focusOnNodes = useCallback(
    (ids: string[], scale?: number) => {
      if (scale != null) centerCameraOnNodes(ids, scale);
      else fitCameraToNodes(ids);
      setBlinkIds(new Set(ids));
    },
    [fitCameraToNodes, centerCameraOnNodes],
  );

  // Task ids covering a given plan section — the section→tasks mapping for nav.
  const sectionTaskIds = useCallback(
    (planId: string, sectionId: string) => {
      const plan = planById.get(planId);
      if (!plan) return [];
      return plan.tasks.filter((t) => t.section_ids.includes(sectionId)).map((t) => t.id);
    },
    [planById],
  );

  // §32.7 section highlight: single-click a section row → highlight its outgoing
  // task edges + connected task nodes (NO camera move). Auto-clears.
  const [highlight, setHighlight] = useState<{ sectionId: string; taskIds: Set<string> } | null>(null);
  const onSectionClick = useCallback(
    (a: SectionNavArgs) => {
      const ids = sectionTaskIds(a.planId, a.sectionId);
      setHighlight({ sectionId: a.sectionId, taskIds: new Set(ids) });
    },
    [sectionTaskIds],
  );
  useEffect(() => {
    if (!highlight) return;
    const h = setTimeout(() => setHighlight(null), 5000);
    return () => clearTimeout(h);
  }, [highlight]);

  // Double-click / row-action → FRAME the connected tasks (fly+blink). Empty
  // section → frame the plan (user can then compose against it).
  const onSectionFrame = useCallback(
    (a: SectionNavArgs) => {
      const ids = sectionTaskIds(a.planId, a.sectionId);
      if (ids.length) focusOnNodes(ids);
      else fitCameraToNodes([a.planId]);
      setHighlight({ sectionId: a.sectionId, taskIds: new Set(ids) });
    },
    [sectionTaskIds, focusOnNodes, fitCameraToNodes],
  );

  // Copy a stable id/link to the clipboard (task, section, stride). User-initiated.
  const onCopyLink = useCallback((id: string) => {
    if (typeof navigator !== "undefined" && navigator.clipboard) void navigator.clipboard.writeText(id).catch(() => {});
  }, []);

  // Focus a stride → frame its owning task (strides live inside the task interior).
  const onFocusStride = useCallback(
    (milestoneId: string) => {
      const taskId = lookups.milestoneTaskById.get(milestoneId);
      if (taskId) focusOnNodes([taskId]);
    },
    [lookups, focusOnNodes],
  );

  // The active plan to land on at mount: an owned plan if any, else the first.
  const activePlanId = useMemo(
    () => (filteredPlans.find((p) => p.owner.pane_id) ?? filteredPlans[0])?.id ?? null,
    [filteredPlans],
  );

  const paneTaskIds = useMemo(
    () => (focusPaneId ? assignedTaskIdsForPane(filteredPlans, focusPaneId) : []),
    [focusPaneId, filteredPlans],
  );

  // §31: auto-fit runs ONLY on mount / when the mount CONTEXT changes (mode or the
  // focused pane) — NEVER on a data change. A new task/plan/stride updates nodes in
  // place; the camera transform stays byte-identical (no surprise pan/recenter).
  const fittedRef = useRef<string | null>(null);
  const fitContextKey = `${mode}:${focusPaneId ?? ""}`;
  useEffect(() => {
    if (size.width <= 0 || size.height <= 0 || layout.bounds.width <= 0) return;
    if (fittedRef.current === fitContextKey) return;
    fittedRef.current = fitContextKey;
    // Task context -> fly to assigned task(s) on the full board. Normal Plan open
    // frames the active plan (§23.4), not fit-all (which lands at 5% on a big board).
    if (paneTaskIds.length) focusOnNodes(paneTaskIds);
    else if (activePlanId) fitCameraToNodes([activePlanId]);
    else fitToView(layout.bounds);
  }, [fitContextKey, fitToView, fitCameraToNodes, focusOnNodes, paneTaskIds, activePlanId, layout.bounds, size.width, size.height]);

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

  // §31: split changed plans into on-screen (pulse in place) vs off-screen
  // (directional edge marker). Camera never moves on a data change — the marker is
  // the only signal; clicking it is a USER-initiated fly-there.
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

  // Esc backs out of a camera-focus (un-dim, release lock).
  useEffect(() => {
    if (!focusedNodeId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setFocusedNodeId(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focusedNodeId]);

  // Double-click a node → camera-focus it (frame + lock Document + dim the rest).
  const onWorldDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      const el = (e.target as HTMLElement).closest("[data-plan-id],[data-task-id]") as HTMLElement | null;
      const id = el?.getAttribute("data-plan-id") ?? el?.getAttribute("data-task-id");
      if (id) focusNodeCamera(id);
    },
    [focusNodeCamera],
  );

  const focusedTitle = focusedNodeId
    ? planById.get(focusedNodeId)?.title ?? lookups.taskById.get(focusedNodeId)?.name ?? null
    : null;

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
            data-layout-revision={layoutRevision}
            onDoubleClick={onWorldDoubleClick}
            style={{
              transform: formatLayerTransform(transform),
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
                  const rowEdge = edge.kind === "section-task" || edge.kind === "spec-row";
                  // ALL edges draw at rest as subtle accent strokes (nothing
                  // floats); hovering a task lifts its own edges and dims the rest
                  // so the row↔task / row↔spec mapping reads at a glance.
                  const inHighlight = highlight != null && (edge.id.startsWith(`${highlight.sectionId}~`) || (edge.to != null && highlight.taskIds.has(edge.to)));
                  const tagRelated = tagFilterActive && edge.to != null && tagFilterMatchIds!.has(edge.to);
                  const related = (hoveredTask != null && (edge.from === hoveredTask || edge.to === hoveredTask)) || inHighlight || tagRelated;
                  const dimmed = (hoveredTask != null || highlight != null || tagFilterActive) && !related;
                  // Per-edge jitter so near-parallel row beziers fan out and each
                  // connection is individually traceable (deterministic).
                  const j = rowEdge ? edgeJitter(edge.id) : 0.5;
                  const curve = DEFAULT_CANVAS_SETTINGS.edgeCurve * (0.65 + 0.8 * j);
                  const baseW = rowEdge ? 1.6 : chain ? 1.2 : 1.4;
                  const baseO = (rowEdge ? 0.42 : chain ? 0.3 : 0.36) * (rowEdge ? 0.82 + 0.36 * j : 1);
                  const showDots = rowEdge || edge.kind === "task-cluster";
                  return (
                    <g key={edge.id}>
                      <path
                        d={bezierPath(ends.a, ends.b, curve)}
                        fill="none"
                        stroke="var(--ct-border-focus)"
                        strokeWidth={related ? 2.6 : baseW}
                        strokeLinecap="round"
                        opacity={related ? 0.95 : dimmed ? 0.12 : baseO}
                      />
                      {rowEdge && (
                        <circle
                          cx={ends.a.x}
                          cy={ends.a.y}
                          r={related ? 4.5 : 3}
                          fill="var(--ct-border-focus)"
                          opacity={related ? 1 : dimmed ? 0.3 : 0.75}
                        />
                      )}
                      {showDots && (
                        <circle
                          cx={ends.b.x}
                          cy={ends.b.y}
                          r={related ? 4.5 : 3}
                          fill="var(--ct-border-focus)"
                          opacity={related ? 1 : dimmed ? 0.3 : 0.75}
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
                .map((n) => {
                  const nodeWorld = worldByNode.get(n.id) ?? "W1";
                  const tagFilterDimmed = tagFilterActive && n.kind === "task" && !tagFilterMatchIds!.has(n.id);
                  const tagFilterHighlighted = tagFilterActive && n.kind === "task" && tagFilterMatchIds!.has(n.id);
                  return (
                  <CanvasNodeCard
                    key={n.id}
                    node={n}
                    model={resolveModel(n)}
                    lod={lod}
                    world={nodeWorld}
                    crispScale={crispRasterScale(nodeWorld, transform.scale)}
                    elevated={dominantNodeId != null && n.id === dominantNodeId}
                    onOpenPane={onOpenPane}
                    onHoverTask={onHoverTask}
                    onArchive={n.kind === "plan" && api ? () => handleArchive(n.id) : undefined}
                    onFitNode={n.kind === "plan" ? fitNode : undefined}
                    onCopyLink={onCopyLink}
                    onFocusStride={onFocusStride}
                    onSectionClick={onSectionClick}
                    onSectionFrame={onSectionFrame}
                    tagFilterDimmed={tagFilterDimmed}
                    tagFilterHighlighted={tagFilterHighlighted}
                  />
                  );
                }),
            )}
            {hoveredTask && lookups.nodeById.has(hoveredTask) && nodeIds.has(hoveredTask) && (() => {
              const box = lookups.nodeById.get(hoveredTask)!.box;
              return (
                <div
                  className="ct-canvas-hovered pointer-events-none absolute"
                  style={{ left: box.x, top: box.y, width: box.width, height: box.height, borderRadius: "var(--ct-radius-md)" }}
                />
              );
            })()}
            {/* §32.7 section highlight: connected task nodes get a steady ring (no
                camera move) so a single-clicked section's tasks read at a glance. */}
            {highlight && [...highlight.taskIds].map((id) => {
              const box = lookups.nodeById.get(id)?.box;
              if (!box || !nodeIds.has(id)) return null;
              return (
                <div
                  key={`hl-${id}`}
                  data-testid="canvas-section-highlight"
                  className="ct-canvas-hovered pointer-events-none absolute"
                  style={{ left: box.x, top: box.y, width: box.width, height: box.height, borderRadius: "var(--ct-radius-md)" }}
                />
              );
            })}
            {tagFilterMatchIds && [...tagFilterMatchIds].map((id) => {
              const box = lookups.nodeById.get(id)?.box;
              if (!box || !nodeIds.has(id)) return null;
              return (
                <div
                  key={`tag-hl-${id}`}
                  data-testid="canvas-tag-filter-highlight"
                  className="ct-canvas-hovered pointer-events-none absolute"
                  style={{ left: box.x, top: box.y, width: box.width, height: box.height, borderRadius: "var(--ct-radius-md)" }}
                />
              );
            })}
            {[...blinkIds].map((id) => {
              const box = lookups.nodeById.get(id)?.box;
              if (!box || !nodeIds.has(id)) return null;
              // Outer ring/glow ONLY (no overlay over the card) — text stays readable.
              return (
                <div
                  key={`blink-${id}`}
                  className="ct-canvas-blink pointer-events-none absolute"
                  style={{ left: box.x, top: box.y, width: box.width, height: box.height, borderRadius: "var(--ct-radius-lg)" }}
                />
              );
            })}
            {/* §31 in-place pulse: a live-changed plan that's on-screen pulses gently
                (no camera move). Ring/glow only, over the plan node box. */}
            {[...pulsePlanIds].map((id) => {
              const box = lookups.nodeById.get(id)?.box;
              if (!box || !nodeIds.has(id)) return null;
              return (
                <div
                  key={`pulse-${id}`}
                  data-testid="canvas-change-pulse"
                  className="ct-canvas-pulse pointer-events-none absolute"
                  style={{ left: box.x, top: box.y, width: box.width, height: box.height, borderRadius: "var(--ct-radius-lg)" }}
                />
              );
            })}
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
      {focusedNodeId && (
        <div
          data-testid="canvas-breadcrumb"
          className="absolute top-2 left-2 z-20 flex items-center gap-1.5 rounded px-2.5 py-1 text-xs"
          style={{ background: "color-mix(in srgb, var(--ct-titlebar-bg) 92%, transparent)", border: "1px solid var(--ct-border-default)", color: "var(--ct-fg)", backdropFilter: "blur(8px)" }}
        >
          <span style={{ opacity: "var(--ct-text-secondary-opacity)" }}>Workspace</span>
          <span style={{ opacity: 0.5 }}>▸</span>
          <span style={{ fontWeight: 600, maxWidth: 220, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{focusedTitle ?? "node"}</span>
          <button type="button" data-testid="canvas-breadcrumb-back" title="Back (Esc)" onClick={() => setFocusedNodeId(null)} style={{ marginLeft: 4, padding: "0 6px", borderRadius: 4, border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer" }}>
            Esc
          </button>
        </div>
      )}
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
  const { plans, loading, error } = useCanvasPlans(mode, projectPath);
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
      focusedPaneId={state.focused_pane_id}
      projectPath={projectPath}
      onClose={onClose}
      onOpenPane={setFocusedPane}
      api={api}
    />
  );
}
