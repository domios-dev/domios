import { useEffect, useState } from "react";
import { shortModelName } from "../../../lib/agentModelName";
import type { ApiConfig } from "../../../lib/types";
import { authHeaders, baseUrl } from "../../MemoryPanel/memoryApi";
import type { PlanStatus } from "../types";
import { UNASSIGNED_MODEL_LABEL, type ModelSpendRow } from "./missionControlBoardModel";
import {
  dayLabel,
  startOfToday,
  type SpendChartData,
  type SpendChartDay,
  type SpendChartSegment,
  type SpendChartSeries,
} from "./spendChart";

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

/** Weekly allowance belongs to a provider account, or to this machine when no account is known. */
export interface TeamWeeklyLimitScope {
  kind: "provider_account" | "machine";
  provider: string;
  account_id: string | null;
}

export interface TeamWeeklyLimitPoint {
  provider: string;
  account_id: string | null;
  weekly_limit_points: number;
  captured_at: number;
  weekly_resets_at: number | null;
  scope: TeamWeeklyLimitScope;
  team_attribution: string;
  model_attribution: string;
}

/** Points one provider account used on one day; never split by model. */
export interface TeamWeeklyLimitDay {
  day: string;
  provider: string;
  account_id: string | null;
  points_used: number;
  weekly_limit_points: number;
  scope: TeamWeeklyLimitScope;
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
  enable_setting?: string | null;
  plans: TeamSpendPlan[];
  attribution: {
    team_plan_count: number;
    unknown_plan_rows: number;
    unassigned_plan_rows: number;
    foreign_plan_rows_excluded: number;
  };
  daily: TeamSpendDaily[];
  weekly_limit_points: TeamWeeklyLimitPoint[];
  weekly_limit_days: TeamWeeklyLimitDay[];
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

/** Why the ledger has nothing to show; null while it records or simply has no rows yet. */
export function teamSpendNote(spend: TeamSpend): string | null {
  if (spend.state !== "not_recording") return null;
  if (spend.not_recording_reason === "disabled") {
    return spend.enable_setting ? `not recording, enable ${spend.enable_setting}` : "not recording";
  }
  if (spend.not_recording_reason === "ledger_unreadable") return "ledger unavailable";
  return null;
}

const UNASSIGNED_KEY = "unassigned";

const NEW_INPUT_SERIES: SpendChartSeries = { key: "new-input", label: "new input", tone: 0 };

const WEEKLY_POINTS_SERIES: SpendChartSeries = { key: "weekly-points", label: "weekly points", tone: 0 };

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

function rangeDayKeys(spend: TeamSpend): string[] {
  const keys: string[] = [];
  for (let start = dayKeyStart(spend.from); start <= dayKeyStart(spend.to); ) {
    keys.push(localDayKey(start));
    const next = new Date(start);
    next.setDate(next.getDate() + 1);
    start = next.getTime();
  }
  return keys.slice(-31);
}

function chartDay(key: string, segments: SpendChartSegment[], todayKey: string): SpendChartDay {
  const start = dayKeyStart(key);
  return {
    start,
    label: dayLabel(start),
    today: key === todayKey,
    segments,
    total: segments.reduce((sum, segment) => sum + segment.value, 0),
  };
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

export interface TeamSpendTotals {
  newInput: number;
  generated: number;
  turns: number;
}

/** The team's new input, generated tokens and turns across its plans for the read period. */
export function teamSpendTotals(spend: TeamSpend): TeamSpendTotals {
  return teamPlanRows(spend).reduce<TeamSpendTotals>(
    (totals, row) => ({
      newInput: totals.newInput + row.uncached_input_tokens,
      generated: totals.generated + row.output_tokens,
      turns: totals.turns + row.turns,
    }),
    { newInput: 0, generated: 0, turns: 0 },
  );
}

export interface TeamPlanShare {
  planId: string;
  title: string;
  newInput: number;
  sharePct: number;
}

/** Each plan that spent in the period with its share of the team's new input, largest first. */
export function teamPlanShares(spend: TeamSpend): TeamPlanShare[] {
  const byPlan = new Map<string, number>();
  for (const row of teamPlanRows(spend)) {
    const planId = row.plan_id ?? "";
    byPlan.set(planId, (byPlan.get(planId) ?? 0) + row.uncached_input_tokens);
  }
  const total = [...byPlan.values()].reduce((sum, value) => sum + value, 0);
  return spend.plans
    .map((plan) => {
      const newInput = byPlan.get(plan.plan_id) ?? 0;
      return { planId: plan.plan_id, title: plan.title, newInput, sharePct: total > 0 ? (newInput / total) * 100 : 0 };
    })
    .filter((share) => share.newInput > 0)
    .sort((a, b) => b.newInput - a.newInput);
}

/** New input the team spent per day: one bar, or a segment per model ranked by new input. */
export function teamTokensChart(spend: TeamSpend, byModel: boolean): SpendChartData {
  const rows = teamPlanRows(spend);
  const todayKey = localDayKey(startOfToday());
  const perModel = new Map<string, { label: string; value: number }>();
  for (const row of rows) {
    const key = modelKey(row.model);
    const entry = perModel.get(key) ?? { label: modelLabel(row.model), value: 0 };
    entry.value += row.uncached_input_tokens;
    perModel.set(key, entry);
  }
  const models = [...perModel.entries()]
    .filter(([, entry]) => entry.value > 0)
    .sort((a, b) => b[1].value - a[1].value)
    .map<SpendChartSeries>(([key, entry], tone) => ({ key, label: entry.label, tone }));

  const days = rangeDayKeys(spend).map((key) => {
    const dayRows = rows.filter((row) => row.day === key);
    const newInput = (match: (row: TeamSpendDaily) => boolean) =>
      dayRows.filter(match).reduce((sum, row) => sum + row.uncached_input_tokens, 0);
    const segments = (byModel
      ? models.map((series) => ({ ...series, value: newInput((row) => modelKey(row.model) === series.key) }))
      : [{ ...NEW_INPUT_SERIES, value: newInput(() => true) }]
    ).filter((segment) => segment.value > 0);
    return chartDay(key, segments, todayKey);
  });
  return { days, legend: byModel ? models : [] };
}

/** Weekly allowance points used per day, summed over the provider accounts the route read. */
export function teamWeeklyChart(weekly: TeamSpend): SpendChartData {
  const todayKey = localDayKey(startOfToday());
  const days = rangeDayKeys(weekly).map((key) => {
    const used = weekly.weekly_limit_days
      .filter((day) => day.day === key)
      .reduce((sum, day) => sum + day.points_used, 0);
    return chartDay(key, used > 0 ? [{ ...WEEKLY_POINTS_SERIES, value: used }] : [], todayKey);
  });
  return { days, legend: [] };
}

export interface WeeklyUsed {
  key: string;
  label: string;
  pct: number;
}

/** The latest weekly allowance used per provider account; an account id only names it when a provider has several. */
export function teamWeeklyUsed(weekly: TeamSpend): WeeklyUsed[] {
  const perProvider = new Map<string, number>();
  for (const point of weekly.weekly_limit_points) {
    perProvider.set(point.provider, (perProvider.get(point.provider) ?? 0) + 1);
  }
  return weekly.weekly_limit_points.map((point) => ({
    key: `${point.provider}-${point.account_id ?? point.scope.kind}`,
    label:
      (perProvider.get(point.provider) ?? 0) > 1 && point.account_id
        ? `${point.provider} ${point.account_id}`
        : point.provider,
    pct: point.weekly_limit_points,
  }));
}

/** Reads one team spend metric for the last week; null until read, or when the team has no reading. */
export function useTeamSpend(
  api: ApiConfig | undefined,
  teamId: string | null | undefined,
  revision: string,
  metric: TeamSpendMetric = "tokens",
): TeamSpend | null {
  const [spend, setSpend] = useState<TeamSpend | null>(null);
  useEffect(() => {
    if (!api || !teamId) {
      setSpend(null);
      return;
    }
    const controller = new AbortController();
    void fetchTeamSpend(api, teamId, { metric, ...teamSpendRange() }, controller.signal)
      .then(setSpend)
      .catch(() => {
        if (!controller.signal.aborted) setSpend(null);
      });
    return () => controller.abort();
  }, [api, teamId, revision, metric]);
  return spend;
}
