import type { AnalyticsAllowance } from "@codeterm/plugin-sdk";
import { classify, type UsageState } from "../../../../../src/types/usage";
import type { FilterState } from "./query";

export interface AllowanceWindow {
  key: string;
  label: string;
  pct: number;
  resetsAt: number | null;
  state: UsageState;
}

export interface AllowanceAccount {
  provider: string;
  label: string;
  capturedAt: number;
  staleReason: string | null;
  windows: AllowanceWindow[];
  sessionMissing: boolean;
}

function window(provider: string, label: string, variant: "session" | "weekly", pct: number | null, resetsAt: number | null): AllowanceWindow[] {
  if (pct === null || !Number.isFinite(pct)) return [];
  return [{ key: `${provider}-${variant}`, label: `${label} ${variant}`, pct, resetsAt, state: classify(pct) }];
}

/** Providers report allowance per account; no figure here is ever split by model, workspace, team or plan. */
export function allowanceAccounts(entries: readonly AnalyticsAllowance[], labels: ReadonlyMap<string, string>): AllowanceAccount[] {
  return entries.map(({ provider, allowance }) => {
    const label = labels.get(provider) ?? provider;
    const session = window(provider, label, "session", allowance.session_used_pct, allowance.session_resets_at);
    const weekly = window(provider, label, "weekly", allowance.weekly_used_pct, allowance.weekly_resets_at);
    return {
      provider,
      label,
      capturedAt: allowance.captured_at,
      staleReason: allowance.stale_reason,
      windows: [...session, ...weekly],
      sessionMissing: session.length === 0,
    };
  });
}

/** Providers that recorded turns in range but hold no allowance snapshot. */
export function silentProviders(providerIds: readonly string[], entries: readonly AnalyticsAllowance[], labels: ReadonlyMap<string, string>): string[] {
  const reporting = new Set(entries.map((entry) => entry.provider));
  return providerIds.filter((id) => !reporting.has(id)).map((id) => labels.get(id) ?? id);
}

/** Only a provider filter chooses accounts; every other filter leaves the whole account in view. */
export function filtersBeyondProvider(filters: FilterState): boolean {
  return filters.models.length + filters.workspaces.length + filters.teams.length + filters.plans.length > 0;
}
