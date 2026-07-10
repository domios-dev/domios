import { useCallback, useMemo, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { archivePlan, renamePlan, unarchivePlan } from "../plansApi";
import type { Plan } from "../types";
import type { PlanKebabAction, PlanSwitcherOption } from "./PlanSwitcher";

function activePlansFrom(
  plans: readonly Plan[],
  localArchivedIds: ReadonlySet<string>,
  localUnarchivedIds: ReadonlySet<string>,
): Plan[] {
  return plans.filter((p) => {
    if (localArchivedIds.has(p.id)) return false;
    if (localUnarchivedIds.has(p.id)) return true;
    return p.status === "active";
  });
}

export function usePlanSwitcherActions(plans: readonly Plan[], api?: ApiConfig | null) {
  const [localArchivedIds, setLocalArchivedIds] = useState<ReadonlySet<string>>(new Set());
  const [localUnarchivedIds, setLocalUnarchivedIds] = useState<ReadonlySet<string>>(new Set());
  const [localTitleOverrides, setLocalTitleOverrides] = useState<ReadonlyMap<string, string>>(new Map());

  const effectivePlans = useMemo(
    () => activePlansFrom(plans, localArchivedIds, localUnarchivedIds),
    [plans, localArchivedIds, localUnarchivedIds],
  );

  const switcherOptions = useMemo<PlanSwitcherOption[]>(
    () =>
      plans.map((p) => ({
        id: p.id,
        title: localTitleOverrides.get(p.id) ?? (p.title.trim() || "Untitled plan"),
        archived: localArchivedIds.has(p.id)
          ? true
          : localUnarchivedIds.has(p.id)
            ? false
            : p.status === "archived",
      })),
    [plans, localArchivedIds, localUnarchivedIds, localTitleOverrides],
  );

  const handleAction = useCallback(
    (action: PlanKebabAction) => {
      if (action.kind === "rename") {
        const plan = plans.find((p) => p.id === action.planId);
        const current =
          localTitleOverrides.get(action.planId) ?? plan?.title.trim() ?? "Untitled plan";
        const next = window.prompt("Plan name:", current);
        if (!next || next.trim() === current) return;
        const trimmed = next.trim();
        setLocalTitleOverrides((m) => new Map(m).set(action.planId, trimmed));
        if (api) {
          renamePlan(api, action.planId, trimmed).catch(() =>
            setLocalTitleOverrides((m) => {
              const n = new Map(m);
              n.delete(action.planId);
              return n;
            }),
          );
        }
      } else if (action.kind === "archive") {
        setLocalArchivedIds((s) => new Set(s).add(action.planId));
        setLocalUnarchivedIds((s) => {
          const n = new Set(s);
          n.delete(action.planId);
          return n;
        });
        if (api) {
          archivePlan(api, action.planId).catch(() =>
            setLocalArchivedIds((s) => {
              const n = new Set(s);
              n.delete(action.planId);
              return n;
            }),
          );
        }
      } else if (action.kind === "unarchive") {
        setLocalUnarchivedIds((s) => new Set(s).add(action.planId));
        setLocalArchivedIds((s) => {
          const n = new Set(s);
          n.delete(action.planId);
          return n;
        });
        if (api) {
          unarchivePlan(api, action.planId).catch(() =>
            setLocalUnarchivedIds((s) => {
              const n = new Set(s);
              n.delete(action.planId);
              return n;
            }),
          );
        }
      }
    },
    [api, plans, localTitleOverrides],
  );

  return { effectivePlans, switcherOptions, handleAction };
}
