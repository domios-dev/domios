import { describe, expect, it } from "vitest";
import type { AnalyticsGroup } from "@codeterm/plugin-sdk";
import { allowanceAccounts, filtersBeyondProvider, silentProviders } from "./allowance";
import { breakdown } from "./breakdown";
import { bucketReading, chartData } from "./chart";
import { DAY, RECORDING_BEGAN, snapshotFor, START, totals } from "./fixtures.testdata";
import { formatClock } from "./format";
import { groupLabels, keyId, keyLabel, toneMap } from "./keys";
import { modelOptions, onlyUnrecorded, providerOptions, recordOptions } from "./options";
import { activeFilterCount, buildQuery, NO_FILTERS, sameModel, toggled } from "./query";
import { rangeFor } from "./range";
import { dashboardView } from "./view";

const NOW_MS = (START + 2 * DAY + 3_600) * 1000;

function named(id: string): AnalyticsGroup {
  return { key: { dimension: "workspace", id }, label: id.toUpperCase(), totals: totals(1) };
}

describe("range and query", () => {
  it("uses day buckets up to 31 days and weeks beyond", () => {
    const now = new Date(2026, 8, 15, 11, 24);
    expect(rangeFor("today", now)).toEqual({ from: "2026-09-15", to: "2026-09-15", bucket: "day" });
    expect(rangeFor("7d", now)).toEqual({ from: "2026-09-09", to: "2026-09-15", bucket: "day" });
    expect(rangeFor("30d", now).bucket).toBe("day");
    expect(rangeFor("90d", now)).toEqual({ from: "2026-06-18", to: "2026-09-15", bucket: "week" });
  });

  it("sends only chosen filters, keeps a null key selectable and pins machine scope", () => {
    const filters = { ...NO_FILTERS, plans: [null, "pl_a"], models: [{ provider: "beta", model: null }] };
    const query = buildQuery({ from: "2026-09-09", to: "2026-09-15", bucket: "day" }, "Europe/Riga", "plan", filters);
    expect(query).toEqual({
      range: { from: "2026-09-09", to: "2026-09-15", tz: "Europe/Riga" },
      bucket: "day",
      filters: { plans: [null, "pl_a"], models: [{ provider: "beta", model: null }] },
      group_by: "plan",
      scope: { kind: "machine" },
    });
    expect(activeFilterCount(filters)).toBe(3);
    expect(toggled(filters.models, { provider: "beta", model: null }, sameModel)).toEqual([]);
  });
});

describe("keys and tones", () => {
  it("names null and deleted keys by their reason and never invents a name", () => {
    expect(keyLabel({ dimension: "model", id: null, provider: "beta", reason: "model_not_reported" }, null)).toBe("Model not reported");
    expect(keyLabel({ dimension: "plan", id: "pl_gone", reason: "plan_deleted" }, null)).toBe("Deleted plan · pl_gone");
    expect(keyLabel({ dimension: "team", id: "team-1" }, null)).toBe("team-1");
    expect(keyLabel({ dimension: "workspace", id: "ws-a" }, "Workspace A")).toBe("Workspace A");
  });

  it("gives the first four named keys a tone, folds the rest into Other and hatches null keys", () => {
    const unnamed: AnalyticsGroup = { key: { dimension: "workspace", id: null, reason: "workspace_not_recorded" }, label: null, totals: totals(1) };
    const groups = [named("a"), unnamed, named("b"), named("c"), named("d"), named("e")];
    const tones = toneMap(groups);
    expect(groups.map((group) => tones.get(keyId(group.key)))).toEqual([0, "unnamed", 1, 2, 3, "other"]);
  });
});

describe("chart", () => {
  const snapshot = snapshotFor("provider_model");
  const catalog = snapshot.tokens.groups;

  it("draws unrecorded buckets without a figure, marks where recording began and rolls models up to providers", () => {
    const data = chartData(snapshot, "new_input", toneMap(catalog), groupLabels(catalog), NOW_MS, false);
    expect(data.days.map((day) => day.coverage)).toEqual(["not_recorded", "partial", "recorded"]);
    expect(data.days[0]).toMatchObject({ total: 0, segments: [], note: "not recorded" });
    expect(data.days[1]!.note).toBe(`from ${formatClock(RECORDING_BEGAN * 1000)}`);
    expect(data.days[1]!.segments.map((segment) => [segment.label, segment.tone, segment.value])).toEqual([
      ["Alpha", 0, 200],
      ["Beta", 1, 100],
    ]);
    expect(data.days[2]).toMatchObject({ total: 0, today: true, note: "today" });
    expect(data.legend.map((series) => series.tone)).toEqual([0, 1, "not_recorded"]);
  });

  it("keeps a key's tone when a filter removes the heavier key", () => {
    const filtered = snapshotFor("provider_model");
    filtered.tokens.series[1]!.groups = filtered.tokens.series[1]!.groups.filter((group) => group.key.provider === "beta");
    const data = chartData(filtered, "turns", toneMap(catalog), groupLabels(catalog), NOW_MS, true);
    expect(data.days[1]!.segments).toEqual([{ key: keyId({ dimension: "provider", id: "beta" }), label: "Beta", tone: 1, value: 1 }]);
  });

  it("reads a selected unrecorded bucket as having no figure", () => {
    const data = chartData(snapshot, "new_input", toneMap(catalog), groupLabels(catalog), NOW_MS, false);
    expect(bucketReading(data, snapshot, START * 1000, "new_input")).toMatchObject({ coverage: "not_recorded", value: null, turns: null });
    expect(bucketReading(data, snapshot, (START + DAY) * 1000, "new_input")).toMatchObject({ value: 300, turns: 3 });
  });
});

describe("breakdown", () => {
  it("shares new input across rows and counts turns whose model was not reported", () => {
    const snapshot = snapshotFor("provider_model");
    const result = breakdown(snapshot, toneMap(snapshot.tokens.groups));
    expect(result.rows.map((row) => [row.label, Math.round(row.sharePct), row.tone])).toEqual([
      ["Alpha", 67, 0],
      ["Beta", 33, 1],
    ]);
    expect(result.rows[1]!.children.map((child) => [child.label, child.unnamed])).toEqual([["Model not reported", true]]);
    expect(result).toMatchObject({ totalTurns: 3, unnamedTurns: 1 });
  });
});

describe("dashboard state", () => {
  it("keeps every missing case distinct from a measured zero", () => {
    const recorded = snapshotFor("workspace");
    expect(dashboardView(null, false)).toEqual({ kind: "loading" });
    expect(dashboardView({ kind: "unavailable" }, false)).toEqual({ kind: "host_unavailable" });
    expect(dashboardView({ kind: "refused", message: "no" }, true)).toEqual({ kind: "refused", message: "no" });
    const unreadable = snapshotFor("workspace", { recording: { ...recorded.recording, state: "unavailable", reason: "database is locked" } });
    expect(dashboardView({ kind: "snapshot", snapshot: unreadable }, false)).toMatchObject({ kind: "ledger_unavailable", reason: "database is locked" });
    const off = snapshotFor("workspace", { recording: { ...recorded.recording, state: "disabled", enable_setting: "kpi.enabled" } });
    expect(dashboardView({ kind: "snapshot", snapshot: off }, false)).toMatchObject({ kind: "data", recordingOff: true });
    const empty = snapshotFor("workspace", { recording: { ...recorded.recording, first_recorded_at: null } });
    expect(dashboardView({ kind: "snapshot", snapshot: empty }, false)).toMatchObject({ kind: "nothing_recorded" });
    const noMatch = snapshotFor("workspace", { tokens: { ...recorded.tokens, totals: totals(0) } });
    expect(dashboardView({ kind: "snapshot", snapshot: noMatch }, true)).toMatchObject({ kind: "no_match" });
    expect(dashboardView({ kind: "snapshot", snapshot: recorded }, false)).toMatchObject({ kind: "data", recordingOff: false });
  });
});

describe("allowance and filter choices", () => {
  const catalog = { provider_model: snapshotFor("provider_model"), workspace: snapshotFor("workspace"), plan: snapshotFor("plan") };
  const labels = new Map([["alpha", "Alpha"], ["beta", "Beta"]]);

  it("reads only the windows an account reported and names accounts that reported none", () => {
    const [account] = allowanceAccounts(catalog.provider_model.allowance, labels);
    expect(account!.windows.map((window) => [window.label, window.pct, window.state])).toEqual([
      ["Alpha session", 70, "warn"],
      ["Alpha weekly", 34, "neutral"],
    ]);
    expect(silentProviders(["alpha", "beta"], catalog.provider_model.allowance, labels)).toEqual(["Beta"]);
    expect(filtersBeyondProvider({ ...NO_FILTERS, providers: ["alpha"] })).toBe(false);
    expect(filtersBeyondProvider({ ...NO_FILTERS, workspaces: [null] })).toBe(true);
  });

  it("offers providers by id, models under their provider and record keys including null", () => {
    expect(providerOptions(catalog).map((option) => option.value)).toEqual(["alpha", "beta"]);
    expect(modelOptions(catalog).map((option) => [option.value, option.detail])).toEqual([
      [{ provider: "alpha", model: "a-large" }, "Alpha"],
      [{ provider: "beta", model: null }, "Beta"],
    ]);
    expect(recordOptions(catalog.plan).map((option) => [option.value, option.label])).toEqual([
      [null, "No plan"],
      ["pl_gone", "Deleted plan · pl_gone"],
    ]);
    const unrecorded = recordOptions(
      snapshotFor("workspace", {
        tokens: {
          ...catalog.workspace.tokens,
          groups: [{ key: { dimension: "workspace", id: null, reason: "workspace_not_recorded" }, label: null, totals: totals(1) }],
        },
      }),
    );
    expect(onlyUnrecorded(unrecorded)).toBe(true);
    expect(onlyUnrecorded(recordOptions(catalog.workspace))).toBe(false);
  });
});
