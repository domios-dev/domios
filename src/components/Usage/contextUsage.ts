import { useEffect, useSyncExternalStore } from "react";
import type { RuntimeUsageProjection, UsageSnapshot } from "../../types/usage";
import { contextMeter } from "../Terminal/composerModels";

export interface ContextUsage {
  used: number;
  window: number;
  percent: number;
  /** Session tokens the harness reported for this tab, when it reports them. */
  sessionTokens: number | null;
  costMicros: number | null;
}

/** Whether a usage reading has quota windows or spend to show; without them the bubble shows the context fill instead. */
export function hasQuota(snapshot: UsageSnapshot | null | undefined): boolean {
  return Boolean(snapshot && (snapshot.session_pct != null || snapshot.weekly_pct != null || snapshot.spend_cents != null));
}

/**
 * The context-window fill a harness reported for a tab (`tab.context`: used_tokens / window_tokens), with its session
 * tokens and cost when reported. Nothing reported, nothing returned: the bubble never invents numbers.
 */
export function contextUsage(context: unknown, runtime: RuntimeUsageProjection | null | undefined): ContextUsage | null {
  const meter = contextMeter(context);
  if (!meter) return null;
  const tokens = runtime?.token_usage?.value;
  return {
    ...meter,
    sessionTokens: tokens ? tokens.input_tokens + tokens.output_tokens : null,
    costMicros: tokens?.cost_micros ?? contextCostMicros(context),
  };
}

function contextCostMicros(context: unknown): number | null {
  const cost = context && typeof context === "object" ? (context as { cost_usd?: unknown }).cost_usd : undefined;
  return typeof cost === "number" && Number.isFinite(cost) && cost >= 0 ? Math.round(cost * 1e6) : null;
}

export function formatCostMicros(micros: number): string {
  return `$${(micros / 1e6).toFixed(2)}`;
}

// The usage bubble that shows a tab's context fill claims it, so the separate context meter does not repeat it.
const claimed = new Set<string>();
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((listener) => listener());
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };

export function useClaimContext(tabId: string | undefined, active: boolean): void {
  useEffect(() => {
    if (!tabId || !active) return;
    claimed.add(tabId);
    notify();
    return () => { claimed.delete(tabId); notify(); };
  }, [tabId, active]);
}

export function useContextClaimed(tabId: string): boolean {
  return useSyncExternalStore(subscribe, () => claimed.has(tabId), () => false);
}
