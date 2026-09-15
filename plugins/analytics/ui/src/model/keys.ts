import type { AnalyticsGroup, AnalyticsGroupKey, AnalyticsKeyReason } from "@codeterm/plugin-sdk";
import type { SpendChartTone } from "../../../../../src/components/Canvas/missionControl/spendChart";

const REASON_COPY: Record<AnalyticsKeyReason, string> = {
  provider_not_reported: "Provider not reported",
  model_not_reported: "Model not reported",
  workspace_not_recorded: "Workspace not recorded",
  workspace_deleted: "Deleted workspace",
  no_plan: "No plan",
  plan_deleted: "Deleted plan",
  no_team: "No team",
  team_deleted: "Deleted team",
};

/** Series tones beyond these fold into grey Other. */
export const NAMED_TONES = 4;

export function keyId(key: AnalyticsGroupKey): string {
  const id = key.id === null ? `~${key.reason ?? ""}` : `=${key.id}`;
  return `${key.dimension}|${key.provider ?? ""}|${id}`;
}

/** A null id names nothing; a deleted record keeps its id beside the reason. */
export function keyLabel(key: AnalyticsGroupKey, label: string | null | undefined): string {
  const reason = key.reason ? REASON_COPY[key.reason] ?? key.reason : null;
  if (key.id === null) return reason ?? "Not reported";
  if (reason) return `${reason} · ${key.id}`;
  return label ?? key.id;
}

export function providerKey(provider: string): AnalyticsGroupKey {
  return provider.trim()
    ? { dimension: "provider", id: provider }
    : { dimension: "provider", id: null, reason: "provider_not_reported" };
}

/** The chart stacks by the Breakdown's top level, so a model key rolls up to its provider. */
export function chartKey(key: AnalyticsGroupKey): AnalyticsGroupKey {
  return key.dimension === "model" ? providerKey(key.provider ?? "") : key;
}

export function groupLabels(groups: readonly AnalyticsGroup[]): Map<string, string> {
  const labels = new Map<string, string>();
  const visit = (group: AnalyticsGroup) => {
    labels.set(keyId(group.key), keyLabel(group.key, group.label));
    group.children?.forEach(visit);
  };
  groups.forEach(visit);
  return labels;
}

/** Tones follow the unfiltered ranking, so filtering never repaints a key that stays visible. */
export function toneMap(groups: readonly AnalyticsGroup[]): Map<string, SpendChartTone> {
  const tones = new Map<string, SpendChartTone>();
  let next = 0;
  for (const group of groups) {
    if (group.key.id === null) {
      tones.set(keyId(group.key), "unnamed");
    } else if (next < NAMED_TONES) {
      tones.set(keyId(group.key), next);
      next += 1;
    } else {
      tones.set(keyId(group.key), "other");
    }
  }
  return tones;
}

export function toneFor(tones: ReadonlyMap<string, SpendChartTone>, key: AnalyticsGroupKey): SpendChartTone {
  return tones.get(keyId(key)) ?? (key.id === null ? "unnamed" : "other");
}
