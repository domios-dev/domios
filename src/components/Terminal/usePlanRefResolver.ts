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

export function usePlanRefResolver(
  api: ApiConfig,
  refs: readonly PlanRefDescriptor[],
): PlanRefResolverState {
  const [version, setVersion] = useState(0);
  const [invalidationGeneration, setInvalidationGeneration] = useState(0);
  const [unreachable, setUnreachable] = useState(false);
  const cacheRef = useRef(new Map<string, ResolvedPlanRefView>());
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

  useEffect(() => {
    let active = true;
    if (!api.enabled) return;
    const missing = orderedRefs.filter((ref) => !cacheRef.current.has(keyOf(ref)));
    if (missing.length === 0) return;
    const ctrl = new AbortController();
    void resolvePlanRefs(api, missing.map(toRequest), ctrl.signal)
      .then((resolved) => {
        if (!active) return;
        for (const item of resolved) {
          const request: PlanRefRequest = {
            plan_id: item.plan_id,
            target_id: item.target_id,
            target_kind: item.target_kind,
          };
          cacheRef.current.set(keyOf({
            planId: item.plan_id,
            targetId: item.target_id,
            targetKind: item.target_kind,
          }), {
            ...item,
            key: keyOf({
              planId: item.plan_id,
              targetId: item.target_id,
              targetKind: item.target_kind,
            }),
            request,
          });
        }
        setUnreachable(false);
        setVersion((n) => n + 1);
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
      cacheRef.current.clear();
      if (active) {
        setInvalidationGeneration((n) => n + 1);
        setVersion((n) => n + 1);
      }
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
