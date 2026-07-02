import { useCallback, useEffect, useState } from "react";
import type { ViewportBox, ViewportTransform, CANVAS_MIN_SCALE, CANVAS_MAX_SCALE } from "../useCanvasViewport";
import { resolveFlyToNodeId } from "../flyToTarget";
import type { SectionNavArgs } from "../nodeRegistry";
import type { Plan } from "../types";
import type { CanvasLookups } from "./models";
import type { NavEntry, NavKind } from "./useCanvasNavHistory";

interface FlyToDeps {
  lookups: CanvasLookups;
  planById: Map<string, Plan>;
  size: { width: number; height: number };
  fitToView: (bounds: ViewportBox) => void;
  animateToView: (bounds: ViewportBox, durationMs?: number) => void;
  animateTo: (next: ViewportTransform, durationMs?: number) => void;
  setTransform: (next: ViewportTransform) => void;
  /** Record a navigation hop so Home/Back return through the real fly path. */
  recordNav?: (entry: NavEntry) => void;
}

// After a spec-section bubble flies to the spec node, scroll that node's internal
// document column to the referenced section and pulse it — so "fly to the spec
// section" lands ON the section, not merely on the node. Imperative + best-effort
// (the camera ease + band reveal settle first); guarded so it no-ops headlessly.
function revealSpecSection(planCtx: string, sectionId: string): void {
  if (typeof document === "undefined") return;
  const attempt = (): boolean => {
    const node = document.querySelector<HTMLElement>(`[data-testid="canvas-spec-node"][data-plan-id="${CSS.escape(planCtx)}"]`);
    const row = node?.querySelector<HTMLElement>(`[data-section-id="${CSS.escape(sectionId)}"]`);
    if (!node || !row) return false;
    const scrollBody = row.closest<HTMLElement>("[data-canvas-scroll-body='1']");
    if (scrollBody) {
      const delta = row.getBoundingClientRect().top - scrollBody.getBoundingClientRect().top;
      scrollBody.scrollTop += delta - 8;
    }
    row.classList.add("ct-canvas-pulse");
    setTimeout(() => row.classList.remove("ct-canvas-pulse"), 3400);
    return true;
  };
  let tries = 0;
  const tick = () => {
    if (attempt() || tries++ > 6) return;
    setTimeout(tick, 220);
  };
  setTimeout(tick, 620);
}

// Fly/focus/section-navigation primitives + transient blink & highlight state.
// Camera math (the SINGLE transform) lives in useCanvasViewport — this only frames
// node sets and wires the delegated fly-to click. Behavior-identical to the inline
// CanvasViewInner callbacks it replaces.
export function useCanvasFlyTo({ lookups, planById, size, fitToView, animateToView, animateTo, setTransform, recordNav }: FlyToDeps) {
  const [blinkIds, setBlinkIds] = useState<Set<string>>(() => new Set());

  // A readable crumb label for a flown-to node (task name → plan title → section title).
  const labelFor = useCallback(
    (id: string): string =>
      lookups.taskById.get(id)?.name ?? planById.get(id)?.title ?? lookups.sectionById.get(id)?.title ?? id,
    [lookups, planById],
  );
  const record = useCallback(
    (id: string, kind: NavKind, flyIds: string[], label?: string) => recordNav?.({ id, kind, label: label ?? labelFor(id), flyIds }),
    [recordNav, labelFor],
  );
  // The arrival blink is transient: the ring pulses then fades so it never leaves a
  // permanent halo, and a later fly to the same node re-mounts → re-triggers it.
  useEffect(() => {
    if (!blinkIds.size) return;
    const h = setTimeout(() => setBlinkIds(new Set()), 3200);
    return () => clearTimeout(h);
  }, [blinkIds]);

  // Move the camera to frame a set of node ids (no blink) — the shared primitive.
  // `animate` eases there (fly-to gesture); omitted = instant (mount / preset).
  const fitCameraToNodes = useCallback(
    (ids: string[], animate = false) => {
      const boxes = ids.map((id) => lookups.nodeById.get(id)?.box).filter(Boolean) as {
        x: number; y: number; width: number; height: number;
      }[];
      if (!boxes.length) return;
      const minX = Math.min(...boxes.map((b) => b.x));
      const minY = Math.min(...boxes.map((b) => b.y));
      const maxX = Math.max(...boxes.map((b) => b.x + b.width));
      const maxY = Math.max(...boxes.map((b) => b.y + b.height));
      // Inflate the framed bbox so a focused node sits fully inside the viewport
      // with margin (no title/edge clipped at the viewport border).
      const w = maxX - minX;
      const h = maxY - minY;
      const padX = w * 0.12;
      const padY = h * 0.18;
      const bounds = { x: minX - padX, y: minY - padY, width: w + padX * 2, height: h + padY * 2 };
      if (animate) animateToView(bounds);
      else fitToView(bounds);
    },
    [lookups, fitToView, animateToView],
  );

  // Center the camera on a node set at an EXPLICIT scale (vs fit-to-bbox). The
  // product "go to node id at zoom Z" primitive: deterministic landing point.
  const centerCameraOnNodes = useCallback(
    (ids: string[], scale: number, animate = false) => {
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
      const s = Math.max(CANVAS_MIN_SCALE, Math.min(CANVAS_MAX_SCALE, scale));
      const t = { x: size.width / 2 - cx * s, y: size.height / 2 - cy * s, scale: s };
      if (animate) animateTo(t);
      else setTransform(t);
    },
    [lookups, setTransform, animateTo, size.width, size.height],
  );

  // Camera + blink onto a set of node ids (shared by canvas-focus + Plan-on-pane).
  // scale given → land centered at that exact zoom; omitted → fit the bbox.
  // `animate` flies the camera (user fly-to) instead of an instant jump.
  const focusOnNodes = useCallback(
    (ids: string[], scale?: number, animate = false) => {
      if (scale != null) centerCameraOnNodes(ids, scale, animate);
      else fitCameraToNodes(ids, animate);
      setBlinkIds(new Set(ids));
    },
    [fitCameraToNodes, centerCameraOnNodes],
  );

  // A presence pin → fly to the task its agent is executing (eased + blink), reusing
  // the shared fly primitive. The literal "click an agent, go to its work" gesture.
  const onFlyToTask = useCallback(
    (taskId: string) => {
      if (lookups.nodeById.has(taskId)) {
        focusOnNodes([taskId], undefined, true);
        record(taskId, "task", [taskId]);
      }
    },
    [lookups, focusOnNodes, record],
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
      record(a.sectionId, "section", ids.length ? ids : [a.planId], lookups.sectionById.get(a.sectionId)?.title);
    },
    [sectionTaskIds, focusOnNodes, fitCameraToNodes, record, lookups],
  );

  // Fly-to navigation (north-star liveness): ONE delegated click across BOTH the
  // world layer and the rich-overlay layer. A swarm-roster row flies to its task; a
  // spec/lineage chip flies to the linked node (or its ghost); a task's section
  // control flies to the plan section it's bound to + highlights it. All eased +
  // blink-on-arrival via focusOnNodes(…, animate) — no camera math reinvented here.
  const onCanvasFlyClick = useCallback(
    (e: React.MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target || typeof target.closest !== "function") return;
      const secBtn = target.closest<HTMLElement>("[data-fly-to-kind='task-section']");
      if (secBtn) {
        const taskId = secBtn.closest<HTMLElement>("[data-task-id]")?.getAttribute("data-task-id") ?? null;
        const planId = taskId ? lookups.nodeById.get(taskId)?.planId ?? null : null;
        if (planId && lookups.nodeById.has(planId)) {
          focusOnNodes([planId], undefined, true);
          const secId = (taskId ? lookups.taskById.get(taskId)?.section_ids?.[0] : null) ?? null;
          if (secId) setHighlight({ sectionId: secId, taskIds: new Set(taskId ? [taskId] : []) });
        }
        return;
      }
      const chip = target.closest<HTMLElement>("[data-fly-to-id]");
      if (chip) {
        const kind = chip.getAttribute("data-fly-to-kind") ?? "";
        const rawId = chip.getAttribute("data-fly-to-id") ?? "";
        const planCtx = chip.closest<HTMLElement>("[data-plan-id]")?.getAttribute("data-plan-id") ?? null;
        const nodeId = resolveFlyToNodeId(kind, rawId, planCtx, (id) => lookups.nodeById.has(id));
        if (nodeId) {
          focusOnNodes([nodeId], undefined, true);
          record(nodeId, kind.startsWith("spec") ? "spec" : "node", [nodeId]);
          // A spec-section bubble lands on the spec node, then scrolls/pulses the
          // exact section it derives from (provenance reads on the section itself).
          if (kind === "spec-section" && planCtx && rawId.trim()) revealSpecSection(planCtx, rawId.trim());
        }
        return;
      }
      const row = target.closest<HTMLElement>("[data-testid='canvas-plan-swarm-task']");
      if (row) {
        const id = row.getAttribute("data-task-id");
        if (id && lookups.nodeById.has(id)) {
          focusOnNodes([id], undefined, true);
          record(id, "task", [id]);
        }
      }
    },
    [lookups, focusOnNodes, record],
  );

  // Copy a stable id/link to the clipboard (task, section, stride). User-initiated.
  const onCopyLink = useCallback((id: string) => {
    if (typeof navigator !== "undefined" && navigator.clipboard) void navigator.clipboard.writeText(id).catch(() => {});
  }, []);

  // Focus a stride → frame its owning task (strides live inside the task interior).
  const onFocusStride = useCallback(
    (milestoneId: string) => {
      const taskId = lookups.milestoneTaskById.get(milestoneId);
      if (taskId) {
        focusOnNodes([taskId]);
        record(taskId, "task", [taskId]);
      }
    },
    [lookups, focusOnNodes, record],
  );

  return {
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
  };
}
