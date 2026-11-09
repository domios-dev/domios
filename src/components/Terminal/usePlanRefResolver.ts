import { useEffect, useMemo, useRef, useState } from "react";
import { onResync } from "../../lib/resync";
import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import {
  resolvePlanRefs,
  type PlanRefRequest,
  type PlanRefTargetKind,
  type ResolvedPlanRef,
} from "../Canvas/plansApi";

export interface PlanRefDescriptor {
  planId: string;
  targetId?: string;
  targetKind: PlanRefTargetKind;
}

export interface ResolvedPlanRefView extends ResolvedPlanRef {
  key: string;
  request: PlanRefRequest;
}

export interface PlanRefResolverState {
  refs: Map<string, ResolvedPlanRefView>;
  unreachable: boolean;
}

function keyOf(ref: PlanRefDescriptor | PlanRefRequest): string {
  if ("planId" in ref) return `${ref.targetKind}:${ref.planId}:${ref.targetId ?? ""}`;
  return `${ref.target_kind}:${ref.plan_id}:${ref.target_id ?? ""}`;
}

function toRequest(ref: PlanRefDescriptor): PlanRefRequest {
  return {
    plan_id: ref.planId,
    target_id: ref.targetId,
    target_kind: ref.targetKind,
  };
}

const RESOLUTION_FIELDS = [
  "state",
  "title",
  "plan_title",
  "task_title",
  "task_key",
  "section_title",
  "file_path",
  "canonical_project_path",
  "status",
  "owner_tab_id",
  "owner_session_id",
] as const;

function sameResolution(prev: ResolvedPlanRef, next: ResolvedPlanRef): boolean {
  return RESOLUTION_FIELDS.every((field) => prev[field] === next[field]);
}

function toView(item: ResolvedPlanRef): ResolvedPlanRefView {
  const key = keyOf({
    planId: item.plan_id,
    targetId: item.target_id,
    targetKind: item.target_kind,
  });
  return {
    ...item,
    key,
    request: {
      plan_id: item.plan_id,
      target_id: item.target_id,
      target_kind: item.target_kind,
    },
  };
}

export function usePlanRefResolver(
  api: ApiConfig,
  refs: readonly PlanRefDescriptor[],
): PlanRefResolverState {
  const [version, setVersion] = useState(0);
  const [invalidationGeneration, setInvalidationGeneration] = useState(0);
  const [unreachable, setUnreachable] = useState(false);
  const cacheRef = useRef(new Map<string, ResolvedPlanRefView>());
  const dirtyPlansRef = useRef(new Map<string, number>());
  const invalidateAllRef = useRef(0);
  const fetchedEpochsRef = useRef(new Map<string, string>());
  const orderedRefs = useMemo(() => {
    const seen = new Set<string>();
    const next: PlanRefDescriptor[] = [];
    for (const ref of refs) {
      const key = keyOf(ref);
      if (seen.has(key)) continue;
      seen.add(key);
      next.push(ref);
    }
    return next;
  }, [refs]);
  const orderedRefKey = useMemo(() => orderedRefs.map(keyOf).join("\u001f"), [orderedRefs]);
  const orderedRefsRef = useRef(orderedRefs);
  orderedRefsRef.current = orderedRefs;

  // Resolved content changes replace rendered state, so unchanged chips never flicker through loading during revalidation.
  useEffect(() => {
    let active = true;
    if (!api.enabled) return;
    const allGeneration = invalidateAllRef.current;
    const dirty = new Map(dirtyPlansRef.current);
    const currentRefs = orderedRefsRef.current;
    const epochOf = (ref: PlanRefDescriptor) => `${allGeneration}:${dirty.get(ref.planId) ?? 0}`;
    const targets = currentRefs.filter((ref) => !cacheRef.current.has(keyOf(ref))
      || fetchedEpochsRef.current.get(keyOf(ref)) !== epochOf(ref));
    if (targets.length === 0) return;
    const ctrl = new AbortController();
    void resolvePlanRefs(api, targets.map(toRequest), ctrl.signal)
      .then((resolved) => {
        if (!active) return;
        for (const ref of targets) fetchedEpochsRef.current.set(keyOf(ref), epochOf(ref));
        const visible = new Set(currentRefs.map(keyOf));
        for (const key of cacheRef.current.keys()) {
          if (!visible.has(key)) { cacheRef.current.delete(key); fetchedEpochsRef.current.delete(key); }
        }
        let changed = false;
        for (const item of resolved) {
          const view = toView(item);
          const prev = cacheRef.current.get(view.key);
          if (prev && sameResolution(prev, item)) continue;
          cacheRef.current.set(view.key, view);
          changed = true;
        }
        setUnreachable(false);
        if (changed) setVersion((n) => n + 1);
      })
      .catch((error) => {
        if (!active || ctrl.signal.aborted || (error instanceof Error && error.name === "AbortError")) return;
        setUnreachable(true);
        setVersion((n) => n + 1);
      });
    return () => {
      active = false;
      ctrl.abort();
    };
  }, [api?.enabled, api?.host, api?.port, api?.token, orderedRefKey, invalidationGeneration]);

  useEffect(() => {
    let active = true;
    let offChanged: (() => void) | null = null;
    let offLoaded: (() => void) | null = null;
    const invalidate = (planId?: string) => {
      if (!active) return;
      if (planId) {
        dirtyPlansRef.current.set(planId, (dirtyPlansRef.current.get(planId) ?? 0) + 1);
        if (!orderedRefsRef.current.some((ref) => ref.planId === planId)) return;
      } else invalidateAllRef.current++;
      setInvalidationGeneration((n) => n + 1);
    };
    const offResync = onResync(() => invalidate());
    void listen<{ plan_id?: string }>("plan-changed", ({ payload }) => {
      if (payload.plan_id) invalidate(payload.plan_id);
    }).then((fn) => {
      if (!active) fn();
      else offChanged = fn;
    });
    void listen("plans-loaded", () => invalidate()).then((fn) => {
      if (!active) fn();
      else offLoaded = fn;
    });
    return () => {
      active = false;
      offResync();
      offChanged?.();
      offLoaded?.();
    };
  }, []);

  return useMemo(() => {
    const map = new Map<string, ResolvedPlanRefView>();
    for (const ref of orderedRefs) {
      const key = keyOf(ref);
      const cached = cacheRef.current.get(key);
      if (cached) map.set(key, cached);
    }
    return { refs: map, unreachable };
  }, [orderedRefs, unreachable, version]);
}
