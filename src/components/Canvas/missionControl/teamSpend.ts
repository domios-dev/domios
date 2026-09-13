import { useEffect, useState } from "react";
import { shortModelName } from "../../../lib/agentModelName";
import type { ApiConfig } from "../../../lib/types";
import { authHeaders, baseUrl } from "../../MemoryPanel/memoryApi";
import type { PlanStatus } from "../types";
import { UNASSIGNED_MODEL_LABEL, type ModelSpendRow } from "./missionControlBoardModel";
import { ALL_FILTER, dayLabel, startOfToday, type SpendChartDay, type SpendChartSource } from "./spendChart";

export type TeamSpendMetric = "tokens" | "weekly_limit_points";

export interface TeamSpendPlan {
  plan_id: string;
  title: string;
  status: PlanStatus;
}

export interface TeamSpendDaily {
  day: string;
  team_id: string | null;
  plan_id: string | null;
  plan_title: string | null;
  plan_status: PlanStatus | null;
  provider: string | null;
  model: string | null;
  input_tokens: number;
  uncached_input_tokens: number;
  output_tokens: number;
  turns: number;
}

export interface TeamWeeklyLimitPoint {
  provider: string;
  account_id: string | null;
  weekly_limit_points: number;
  captured_at: number;
  weekly_resets_at: number | null;
  scope: string;
  team_attribution: string;
  model_attribution: string;
}

export interface TeamSpend {
  team_id: string;
  metric: TeamSpendMetric;
  units: string;
  scope: string;
  model: string;
  from: string;
  to: string;
  state: "recording" | "not_recording";
  not_recording_reason: string | null;
  plans: TeamSpendPlan[];
  attribution: {
    team_plan_count: number;
    unknown_plan_rows: number;
    unassigned_plan_rows: number;
    foreign_plan_rows_excluded: number;
  };
  daily: TeamSpendDaily[];
  weekly_limit_points: TeamWeeklyLimitPoint[];
}

export interface TeamSpendQuery {
  metric: TeamSpendMetric;
  model?: string;
  from?: string;
  to?: string;
}

/** Days the Board reads of team spend. */
export const TEAM_SPEND_DAYS = 7;

export const TEAM_SCOPE_CAPTION = "team";

const UNASSIGNED_KEY = "unassigned";

export async function fetchTeamSpend(
  api: ApiConfig,
  teamId: string,
  query: TeamSpendQuery,
  signal?: AbortSignal,
): Promise<TeamSpend> {
  const params = new URLSearchParams({ metric: query.metric });
  if (query.model) params.set("model", query.model);
  if (query.from) params.set("from", query.from);
  if (query.to) params.set("to", query.to);
  const res = await fetch(
    `${baseUrl(api)}/api/teams/${encodeURIComponent(teamId)}/mission-control/spend?${params.toString()}`,
    { headers: authHeaders(api), signal },
  );
  if (!res.ok) throw new Error(`team spend failed: ${res.status} ${await res.text()}`);
  return (await res.json()) as TeamSpend;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

export function localDayKey(ms: number): string {
  const day = new Date(ms);
  return `${day.getFullYear()}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}`;
}

function dayKeyStart(key: string): number {
  const [year, month, date] = key.split("-").map(Number);
  return new Date(year ?? NaN, (month ?? NaN) - 1, date ?? NaN).getTime();
}

export function teamSpendRange(now = Date.now()): { from: string; to: string } {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const from = new Date(today);
  from.setDate(from.getDate() - (TEAM_SPEND_DAYS - 1));
  return { from: localDayKey(from.getTime()), to: localDayKey(today.getTime()) };
}

/** Rows attributed to one of the team's plans; plan-less machine rows never count as team spend. */
export function teamPlanRows(spend: TeamSpend): TeamSpendDaily[] {
  const plans = new Set(spend.plans.map((plan) => plan.plan_id));
  return spend.daily.filter((row) => row.plan_id != null && plans.has(row.plan_id));
}

function modelKey(model: string | null): string {
  return model ?? UNASSIGNED_KEY;
}

function modelLabel(model: string | null): string {
  return model == null ? UNASSIGNED_MODEL_LABEL : shortModelName(model) ?? model;
}

/** The team's models ranked by new input across its plans, with each one's share of the team's new input. */
export function teamModelRows(spend: TeamSpend): ModelSpendRow[] {
  const rows = teamPlanRows(spend);
  const providers = [...new Set(rows.map((row) => row.provider ?? ""))];
  const models = new Map<string, Omit<ModelSpendRow, "sharePct">>();
  for (const row of rows) {
    const key = `${row.provider ?? ""}-${modelKey(row.model)}`;
    const entry = models.get(key) ?? {
      key,
      label: modelLabel(row.model),
      unassigned: row.model == null,
      tone: providers.indexOf(row.provider ?? ""),
      newInput: 0,
    };
    entry.newInput += row.uncached_input_tokens;
    models.set(key, entry);
  }
  const total = [...models.values()].reduce((sum, entry) => sum + entry.newInput, 0);
  return [...models.values()]
    .map((entry) => ({ ...entry, sharePct: total > 0 ? (entry.newInput / total) * 100 : 0 }))
    .sort((a, b) => b.newInput - a.newInput);
}

/** New input the team spent per day, one segment per plan, narrowed to one model on request. */
export function teamTokensChartSource(spend: TeamSpend): SpendChartSource {
  const rows = teamPlanRows(spend);
  const planTone = new Map(spend.plans.map((plan, index) => [plan.plan_id, index]));
  const keys: string[] = [];
  for (let start = dayKeyStart(spend.from); start <= dayKeyStart(spend.to); ) {
    keys.push(localDayKey(start));
    const next = new Date(start);
    next.setDate(next.getDate() + 1);
    start = next.getTime();
  }
  const todayKey = localDayKey(startOfToday());
  const modelFilters = [...new Map(rows.map((row) => [modelKey(row.model), modelLabel(row.model)])).entries()].map(
    ([key, label]) => ({ key, label }),
  );

  const tokenDays = (filter: string): SpendChartDay[] =>
    keys.slice(-31).map((key) => {
      const perPlan = new Map<string, { label: string; value: number }>();
      for (const row of rows) {
        if (row.day !== key || (filter !== ALL_FILTER && modelKey(row.model) !== filter)) continue;
        const planId = row.plan_id ?? "";
        const entry = perPlan.get(planId) ?? { label: row.plan_title ?? planId, value: 0 };
        entry.value += row.uncached_input_tokens;
        perPlan.set(planId, entry);
      }
      const segments = [...perPlan.entries()]
        .filter(([, entry]) => entry.value > 0)
        .map(([planId, entry]) => ({ key: planId, label: entry.label, tone: planTone.get(planId) ?? 0, value: entry.value }));
      return {
        start: dayKeyStart(key),
        label: dayLabel(dayKeyStart(key)),
        today: key === todayKey,
        segments,
        total: segments.reduce((sum, segment) => sum + segment.value, 0),
      };
    });

  return {
    hasData: spend.plans.length > 0,
    metrics: { tokens: spend.plans.length > 0, points: false },
    scope: { tokens: TEAM_SCOPE_CAPTION, points: "" },
    filters: (metric) =>
      metric === "tokens" && modelFilters.length > 1 ? [{ key: ALL_FILTER, label: "All" }, ...modelFilters] : [],
    days: (metric, filter) => (metric === "tokens" ? tokenDays(filter) : []),
  };
}

/** Reads the team's token spend for the last week; null until read, or when the team has no reading. */
export function useTeamSpend(api: ApiConfig | undefined, teamId: string | null | undefined, revision: string): TeamSpend | null {
  const [spend, setSpend] = useState<TeamSpend | null>(null);
  useEffect(() => {
    if (!api || !teamId) {
      setSpend(null);
      return;
    }
    const controller = new AbortController();
    void fetchTeamSpend(api, teamId, { metric: "tokens", ...teamSpendRange() }, controller.signal)
      .then(setSpend)
      .catch(() => {
        if (!controller.signal.aborted) setSpend(null);
      });
    return () => controller.abort();
  }, [api, teamId, revision]);
  return spend;
}
