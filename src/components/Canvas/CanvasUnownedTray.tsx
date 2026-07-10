import { useCallback, useEffect, useState } from "react";
import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import { claimPlan, fetchUnownedPlanIndex, type PlanIndexEntry } from "./plansApi";
import type { OrchestratorRef } from "./types";

export interface CanvasUnownedTrayProps {
  api: ApiConfig;
  claimOwner?: OrchestratorRef;
}

export function CanvasUnownedTray({ api, claimOwner }: CanvasUnownedTrayProps) {
  const [entries, setEntries] = useState<PlanIndexEntry[]>([]);
  const [open, setOpen] = useState(false);
  const [claimingId, setClaimingId] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const next = await fetchUnownedPlanIndex(api);
      setEntries(next);
    } catch {
      setEntries([]);
    }
  }, [api]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void listen<{ plan_id?: string }>("plan-changed", () => {
      if (active) void reload();
    }).then((fn) => {
      if (!active) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [reload]);

  const onClaim = useCallback(
    async (planId: string) => {
      if (!claimOwner || claimingId) return;
      setClaimingId(planId);
      try {
        await claimPlan(api, planId, claimOwner);
        await reload();
      } finally {
        setClaimingId(null);
      }
    },
    [api, claimOwner, claimingId, reload],
  );

  if (entries.length === 0) return null;

  return (
    <div
      data-testid="canvas-unowned-tray"
      className="absolute bottom-3 left-3 z-20 max-w-xs rounded border text-sm"
      style={{
        background: "var(--ct-bg)",
        borderColor: "var(--ct-border)",
        color: "var(--ct-fg)",
        boxShadow: "var(--ct-shadow-md)",
      }}
    >
      <button
        type="button"
        data-testid="canvas-unowned-toggle"
        onClick={() => setOpen((v) => !v)}
        className="w-full px-3 py-2 text-left cursor-pointer"
        style={{ borderBottom: open ? "1px solid var(--ct-border-subtle)" : undefined }}
      >
        Unowned plans ({entries.length})
      </button>
      {open && (
        <ul className="max-h-48 overflow-y-auto py-1">
          {entries.map((e) => (
            <li
              key={e.plan_id}
              data-testid="canvas-unowned-item"
              data-plan-id={e.plan_id}
              className="flex items-center gap-2 px-3 py-1.5"
            >
              <span className="flex-1 truncate" title={e.title}>
                {e.title}
              </span>
              <button
                type="button"
                data-testid="canvas-unowned-claim"
                disabled={!claimOwner || claimingId === e.plan_id}
                onClick={() => void onClaim(e.plan_id)}
                className="shrink-0 rounded border px-2 py-0.5 text-xs cursor-pointer disabled:opacity-50"
                style={{ borderColor: "var(--ct-border-subtle)" }}
              >
                {claimingId === e.plan_id ? "…" : "Claim"}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
