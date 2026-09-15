import type {
  AnalyticsGroup,
  AnalyticsGroupBy,
  AnalyticsSnapshot,
  AnalyticsTokenBucket,
  AnalyticsTotals,
} from "@codeterm/plugin-sdk";

export const HOUR = 3_600;
export const DAY = 24 * HOUR;
export const START = 1_789_000_000;
export const RECORDING_BEGAN = START + DAY + 17 * HOUR;

export function totals(newInput: number, generated = 0, cache = 0, turns = 0): AnalyticsTotals {
  return { new_input: newInput, generated, read_from_cache: cache, turns };
}

const ALPHA: AnalyticsGroup = {
  key: { dimension: "provider", id: "alpha" },
  label: "Alpha",
  totals: totals(200, 20, 600, 2),
  children: [{ key: { dimension: "model", id: "a-large", provider: "alpha" }, label: "a-large", totals: totals(200, 20, 600, 2) }],
};

const BETA: AnalyticsGroup = {
  key: { dimension: "provider", id: "beta" },
  label: "Beta",
  totals: totals(100, 10, 400, 1),
  children: [
    { key: { dimension: "model", id: null, provider: "beta", reason: "model_not_reported" }, label: null, totals: totals(100, 10, 400, 1) },
  ],
};

const GROUPS: Record<AnalyticsGroupBy, AnalyticsGroup[]> = {
  provider_model: [ALPHA, BETA],
  workspace: [
    { key: { dimension: "workspace", id: "ws-a" }, label: "Workspace A", totals: totals(180, 18, 500, 2) },
    { key: { dimension: "workspace", id: "ws-b" }, label: "Workspace B", totals: totals(120, 12, 500, 1) },
  ],
  team: [{ key: { dimension: "team", id: null, reason: "no_team" }, label: null, totals: totals(300, 30, 1000, 3) }],
  plan: [
    { key: { dimension: "plan", id: null, reason: "no_plan" }, label: null, totals: totals(250, 25, 800, 2) },
    { key: { dimension: "plan", id: "pl_gone", reason: "plan_deleted" }, label: null, totals: totals(50, 5, 200, 1) },
  ],
};

function leafGroups(groupBy: AnalyticsGroupBy): AnalyticsTokenBucket["groups"] {
  return GROUPS[groupBy].flatMap((group) => (group.children?.length ? group.children : [group])).map((group) => ({ key: group.key, totals: group.totals }));
}

export function snapshotFor(groupBy: AnalyticsGroupBy, overrides: Partial<AnalyticsSnapshot> = {}): AnalyticsSnapshot {
  return {
    scope: { kind: "machine", includes_mesh_peers: false },
    as_of: START + 3 * DAY - HOUR,
    range: { from: "2026-09-09", to: "2026-09-11", tz: "UTC" },
    bucket: "day",
    group_by: groupBy,
    recording: {
      state: "recording",
      reason: null,
      enable_setting: null,
      first_recorded_at: RECORDING_BEGAN,
      last_recorded_at: START + 3 * DAY - 2 * HOUR,
      workspace_recorded_since: START,
    },
    tokens: {
      totals: totals(300, 30, 1000, 3),
      series: [
        { start: START, end: START + DAY, label: "2026-09-09", coverage: "not_recorded", totals: null, groups: [] },
        { start: START + DAY, end: START + 2 * DAY, label: "2026-09-10", coverage: "partial", totals: totals(300, 30, 1000, 3), groups: leafGroups(groupBy) },
        { start: START + 2 * DAY, end: START + 3 * DAY, label: "2026-09-11", coverage: "recorded", totals: totals(0), groups: [] },
      ],
      groups: GROUPS[groupBy],
    },
    tabs_created: {
      state: "counted",
      counted_since: START,
      total: 4,
      by_kind: { agent: 3, terminal: 1 },
      series: [],
    },
    allowance: [
      {
        provider: "alpha",
        allowance: {
          captured_at: (START + 2 * DAY) * 1000,
          session_used_pct: 70,
          session_resets_at: (START + 3 * DAY) * 1000,
          weekly_used_pct: 34,
          weekly_resets_at: null,
          stale_reason: null,
        },
        attribution: { model: "unavailable", workspace: "unavailable", team: "unavailable", plan: "unavailable" },
      },
    ],
    money: { state: "not_recorded" },
    ...overrides,
  };
}
