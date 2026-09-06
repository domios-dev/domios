import { useEffect, useMemo, useRef, useState } from "react";
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
  const fetchedGenerationRef = useRef(0);
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

  // Stale-while-revalidate: an invalidation refetches every visible ref in the
  // background while the cache keeps serving, and only entries whose resolved
  useEffect(() => {
    let active = true;
    if (!api.enabled) return;
    const generation = invalidationGeneration;
    const revalidate = fetchedGenerationRef.current !== generation;
    const targets = revalidate
      ? orderedRefs
      : orderedRefs.filter((ref) => !cacheRef.current.has(keyOf(ref)));
    if (targets.length === 0) {
      fetchedGenerationRef.current = generation;
      return;
    }
    const ctrl = new AbortController();
    void resolvePlanRefs(api, targets.map(toRequest), ctrl.signal)
      .then((resolved) => {
        if (!active) return;
        fetchedGenerationRef.current = generation;
        if (revalidate) {
          const visible = new Set(orderedRefs.map(keyOf));
          for (const key of [...cacheRef.current.keys()]) {
            if (!visible.has(key)) cacheRef.current.delete(key);
          }
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
  }, [api, orderedRefs, invalidationGeneration]);

  useEffect(() => {
    let active = true;
    let offChanged: (() => void) | null = null;
    let offLoaded: (() => void) | null = null;
    const invalidate = () => {
      if (active) setInvalidationGeneration((n) => n + 1);
    };
    void listen("plan-changed", invalidate).then((fn) => {
      if (!active) fn();
      else offChanged = fn;
    });
    void listen("plans-loaded", invalidate).then((fn) => {
      if (!active) fn();
      else offLoaded = fn;
    });
    return () => {
      active = false;
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
