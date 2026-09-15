import type { AnalyticsGroup, AnalyticsSnapshot, AnalyticsTotals } from "@codeterm/plugin-sdk";
import type { SpendChartTone } from "../../../../../src/components/Canvas/missionControl/spendChart";
import { keyId, keyLabel, toneFor } from "./keys";

export interface BreakdownRow {
  id: string;
  label: string;
  unnamed: boolean;
  tone: SpendChartTone | null;
  totals: AnalyticsTotals;
  sharePct: number;
}

export interface BreakdownParent extends BreakdownRow {
  children: BreakdownRow[];
}

export interface Breakdown {
  rows: BreakdownParent[];
  totalTurns: number;
  /** Turns whose key names nothing: no model reported, no workspace recorded, no plan or team. */
  unnamedTurns: number;
}

/** Top-level rows the table shows before its Show all control. */
export const BREAKDOWN_PREVIEW = 8;

function share(value: number, total: number): number {
  return total > 0 ? (value / total) * 100 : 0;
}

function row(group: AnalyticsGroup, total: number, tone: SpendChartTone | null): BreakdownRow {
  return {
    id: keyId(group.key),
    label: keyLabel(group.key, group.label),
    unnamed: group.key.id === null,
    tone,
    totals: group.totals,
    sharePct: share(group.totals.new_input, total),
  };
}

export function breakdown(snapshot: AnalyticsSnapshot, tones: ReadonlyMap<string, SpendChartTone>): Breakdown {
  const total = snapshot.tokens.totals?.new_input ?? 0;
  let unnamedTurns = 0;
  const rows = snapshot.tokens.groups.map<BreakdownParent>((group) => {
    const children = (group.children ?? []).map((child) => row(child, total, null));
    const leaves = group.children?.length ? group.children : [group];
    unnamedTurns += leaves.filter((leaf) => leaf.key.id === null).reduce((sum, leaf) => sum + leaf.totals.turns, 0);
    return { ...row(group, total, toneFor(tones, group.key)), children };
  });
  return { rows, totalTurns: snapshot.tokens.totals?.turns ?? 0, unnamedTurns };
}
