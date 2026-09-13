import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import type { ApiConfig } from "../../../lib/types";
import type { UsageSnapshot } from "../../../types/usage";
import { authHeaders, baseUrl } from "../../MemoryPanel/memoryApi";
import {
  fetchTeamSpend,
  localDayKey,
  teamPlanRows,
  teamSpendRange,
  type TeamSpend,
  type TeamSpendDaily,
} from "./teamSpend";
import { allowanceWeekDays, type AllowanceDay, type AllowanceWeeks } from "./allowanceWeek";
import { ModelsCard, SpendCard } from "./BoardCards";
import {
  MODELS_PREVIEW,
  SPEND_SCOPE_CAPTION,
  UNASSIGNED_MODEL_LABEL,
  modelSpendRows,
  spendReading,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";
import { allowanceChartSource } from "./spendChart";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const H = 3_600_000;
const NOW = 1_700_000_000_000;

/** The two shapes the live ledger actually holds: a named model and a NULL one. */
function recordingSpend(): MissionControlSpend {
  return {
    scope: "machine",
    state: "recording",
    measured_tokens: 1_000,
    turns: 30,
    not_recording_reason: null,
    enable_setting: null,
    providers: [
      {
        provider: "claude",
        label: "Claude",
        allowance: {
          captured_at: NOW,
          session_used_pct: 58,
          session_resets_at: NOW + 2 * H,
          weekly_used_pct: 34,
          weekly_resets_at: NOW + 48 * H,
          stale_reason: null,
        },
        turns: 20,
        input_tokens: 550,
        uncached_input_tokens: 120,
        output_tokens: 50,
        models: [
          {
            model: "haiku",
            turns: 18,
            input_tokens: 500,
            uncached_input_tokens: 100,
            output_tokens: 40,
            token_share_pct: 54,
          },
          {
            model: "opus",
            turns: 2,
            input_tokens: 50,
            uncached_input_tokens: 20,
            output_tokens: 10,
            token_share_pct: 6,
          },
        ],
      },
      {
        provider: "codex",
        label: "Codex",
        allowance: {
          captured_at: NOW,
          session_used_pct: null,
          session_resets_at: null,
          weekly_used_pct: 81,
          weekly_resets_at: NOW + 72 * H,
          stale_reason: null,
        },
        turns: 10,
        input_tokens: 300,
        uncached_input_tokens: 90,
        output_tokens: 100,
        models: [
          {
            model: null,
            turns: 10,
            input_tokens: 300,
            uncached_input_tokens: 90,
            output_tokens: 100,
            token_share_pct: 40,
          },
        ],
      },
    ],
  };
}

/** Two team plans on two days, and one plan-less machine row that must never count as team spend. */
function teamSpendFixture(): TeamSpend {
  const today = localDayKey(Date.now());
  const yesterday = localDayKey(Date.now() - 24 * H);
  const row = (
    day: string,
    planId: string | null,
    planTitle: string | null,
    provider: string,
    model: string | null,
    uncached: number,
  ): TeamSpendDaily => ({
    day,
    team_id: planId ? "team-1" : null,
    plan_id: planId,
    plan_title: planTitle,
    plan_status: planId ? "active" : null,
    provider,
    model,
    input_tokens: uncached * 2,
    uncached_input_tokens: uncached,
    output_tokens: 1,
    turns: 1,
  });
  return {
    team_id: "team-1",
    metric: "tokens",
    units: "tokens",
    scope: "team_plan",
    model: "all",
    ...teamSpendRange(),
    state: "recording",
    not_recording_reason: null,
    plans: [
      { plan_id: "pl_snake", title: "Snake", status: "active" },
      { plan_id: "pl_old", title: "Old", status: "archived" },
    ],
    attribution: { team_plan_count: 2, unknown_plan_rows: 0, unassigned_plan_rows: 1, foreign_plan_rows_excluded: 0 },
    daily: [
      row(yesterday, "pl_old", "Old", "codex", null, 50),
      row(today, "pl_snake", "Snake", "claude", "claude-haiku-4-5", 100),
      row(today, "pl_old", "Old", "codex", null, 20),
      row(today, null, null, "claude", "claude-haiku-4-5", 9_999),
    ],
    weekly_limit_points: [],
  };
}

function notRecordingSpend(
  reason: MissionControlSpend["not_recording_reason"],
  enableSetting: string | null = null,
): MissionControlSpend {
  return {
    scope: "machine",
    state: "not_recording",
    measured_tokens: 0,
    turns: 0,
    not_recording_reason: reason,
    enable_setting: enableSetting,
    providers: [],
  };
}

async function mount(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return {
    host,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function text(host: HTMLElement, testId: string): string {
  return host.querySelector(`[data-testid="${testId}"]`)?.textContent?.trim() ?? "";
}

function modelCells(host: HTMLElement): string[][] {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-spend-model"]')).map((row) =>
    Array.from(row.children).map((cell) => cell.textContent?.trim() ?? ""),
  );
}

describe("mission control spend card", () => {
  it("mission_control_spend_names_each_provider_once_with_its_allowance_windows", async () => {
    const { host, unmount } = await mount(<SpendCard spend={recordingSpend()} />);

    const providers = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-provider"]'));
    expect(providers.map((provider) => provider.getAttribute("data-provider"))).toEqual(["claude", "codex"]);
    const card = text(host, "mc-board-spend");
    for (const label of ["Claude", "Codex"]) {
      expect(card.split(label)).toHaveLength(2);
    }
    const gauges = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')) as HTMLElement[];
    expect(gauges.map((gauge) => gauge.textContent)).toEqual(["34%", "session 58%", "81%"]);
    const limits = host.querySelector('[data-testid="mc-board-spend-limits"]') as HTMLElement;
    expect(limits.tagName).toBe("UL");
    expect(limits.querySelectorAll("h3")).toHaveLength(0);
    expect(text(host, "mc-board-spend-limits-scope")).toBe("machine");
    expect(
      Array.from(limits.querySelectorAll('[data-testid="mc-board-spend-provider"]')).map((provider) => [
        provider.querySelector(".mc-board-spend-limit-name")?.textContent,
        provider.getAttribute("title"),
      ]),
    ).toEqual([
      ["Claude", "Claude"],
      ["Codex", "Codex"],
    ]);
    expect(gauges[2]!.querySelector("b")?.classList.contains("mc-board-warn")).toBe(true);
    expect(host.querySelector("table")).toBeNull();
    await unmount();
  });

  it("mission_control_spend_keeps_the_machine_scope_with_no_prose_or_placeholder", async () => {
    const { host, unmount } = await mount(<SpendCard spend={recordingSpend()} />);

    const card = host.querySelector('[data-testid="mc-board-spend"]') as HTMLElement;
    expect(Array.from(card.querySelectorAll("h2")).map((heading) => heading.textContent)).toEqual(["Spend"]);
    expect(text(host, "mc-board-spend-scope")).toBe(SPEND_SCOPE_CAPTION);
    expect(card.querySelectorAll("p")).toHaveLength(0);
    const words = (card.textContent ?? "").toLowerCase();
    for (const banned of ["this plan", "output", "total", "bytes", "·", "needs", "spent", "used"]) {
      expect(words).not.toContain(banned);
    }
    await unmount();
  });

  it("mission_control_spend_reports_each_not_recording_state_without_a_dash", async () => {
    for (const [spend, copy] of [
      [notRecordingSpend("disabled", "kpi.enabled"), "not recording, enable kpi.enabled"],
      [notRecordingSpend("unreadable"), "ledger unavailable"],
      [notRecordingSpend("no_rows_for_plan"), "no data"],
    ] as const) {
      const { host, unmount } = await mount(<SpendCard spend={spend} />);
      expect(text(host, "mc-board-spend-empty")).toBe(copy);
      expect(text(host, "mc-board-spend")).not.toContain("—");
      expect(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(0);
      await unmount();
    }
  });

  it("mission_control_spend_keeps_provider_allowance_when_the_ledger_is_unreadable", async () => {
    const measured = recordingSpend();
    const spend: MissionControlSpend = {
      ...notRecordingSpend("unreadable"),
      providers: [{ ...measured.providers[0]!, models: [] }],
    };
    const { host, unmount } = await mount(<SpendCard spend={spend} />);

    expect(text(host, "mc-board-spend-units")).toBe("ledger unavailable");
    expect(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(2);
    await unmount();
  });

  it("mission_control_spend_never_presents_a_model_share_as_allowance", () => {
    const reading = spendReading(recordingSpend());

    expect(reading.groups.map((group) => group.windows.map((window) => window.variant))).toEqual([
      ["weekly", "session"],
      ["weekly"],
    ]);
    for (const row of modelSpendRows(recordingSpend())) {
      expect(Object.keys(row)).not.toContain("windows");
    }
    expect(modelSpendRows(recordingSpend()).reduce((sum, row) => sum + row.sharePct, 0)).toBe(100);
  });

  it("mission_control_spend_charts_limit_points_by_day_and_filters_to_one_provider", async () => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const start = (offset: number) => today.getTime() + offset * 24 * H;
    const day = (offset: number, pct: number | null): AllowanceDay => ({
      start: start(offset),
      label: new Date(start(offset)).toLocaleDateString("en-US", { weekday: "short" }),
      pct,
      today: offset === 0,
    });
    const weeks: AllowanceWeeks = {
      claude: [day(-2, 10), day(-1, 20), day(0, 4), day(1, null)],
      codex: [day(-1, 6), day(0, 2), day(1, null)],
    };
    const { host, unmount } = await mount(<SpendCard spend={recordingSpend()} weeks={weeks} />);

    const chart = host.querySelector('[data-testid="mc-board-spend-chart"]') as HTMLElement;
    const days = () => Array.from(chart.querySelectorAll('[data-testid="mc-board-spend-chart-day"]')) as HTMLElement[];
    const tones = () =>
      days().map((entry) =>
        Array.from(entry.querySelectorAll('[data-testid="mc-board-spend-chart-segment"]')).map((segment) =>
          segment.getAttribute("data-tone"),
        ),
      );
    expect(days()).toHaveLength(3);
    expect(days()[2]!.getAttribute("data-today")).toBe("true");
    expect(tones()).toEqual([["0"], ["0", "1"], ["0", "1"]]);
    expect((days()[1]!.querySelector(".mc-board-chart-stack") as HTMLElement).style.height).toBe("100%");
    expect(chart.querySelector('[role="img"]')?.getAttribute("aria-label")).toMatch(/^limit % by day: \w{3} 10%, \w{3} 26%, \w{3} 6%$/);

    const metric = (key: string) => host.querySelector(`[data-testid="mc-board-spend-metric-${key}"]`) as HTMLButtonElement;
    expect(metric("points").getAttribute("aria-pressed")).toBe("true");
    expect(metric("tokens").disabled).toBe(true);
    const filters = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-filter"]')) as HTMLButtonElement[];
    expect(filters.map((filter) => [filter.textContent, filter.getAttribute("aria-pressed")])).toEqual([
      ["All", "true"],
      ["Claude", "false"],
      ["Codex", "false"],
    ]);
    await act(async () => filters[2]!.click());
    expect(tones()).toEqual([[], ["1"], ["1"]]);
    expect(host.querySelector("input, select, textarea")).toBeNull();
    await unmount();
  });

  it("mission_control_spend_charts_team_new_input_by_plan_and_filters_to_one_model", async () => {
    const { host, unmount } = await mount(<SpendCard spend={recordingSpend()} teamSpend={teamSpendFixture()} />);

    expect(text(host, "mc-board-spend-scope")).toBe("team");
    expect(text(host, "mc-board-spend-chart-scope")).toBe("team");
    expect(host.querySelector('[data-testid="mc-board-spend-limits-label"]')).toBeNull();
    expect(text(host, "mc-board-spend-limits-scope")).toBe("machine");
    expect(text(host, "mc-board-spend-limits")).not.toContain("team");
    const metric = (key: string) => host.querySelector(`[data-testid="mc-board-spend-metric-${key}"]`) as HTMLButtonElement;
    expect(metric("tokens").getAttribute("aria-pressed")).toBe("true");
    expect(metric("points").disabled).toBe(true);

    const days = () => Array.from(host.querySelectorAll('[data-testid="mc-board-spend-chart-day"]')) as HTMLElement[];
    const tones = (day: HTMLElement) =>
      Array.from(day.querySelectorAll('[data-testid="mc-board-spend-chart-segment"]')).map((segment) =>
        segment.getAttribute("data-tone"),
      );
    expect(days()).toHaveLength(7);
    const today = days()[6]!;
    expect(today.getAttribute("data-today")).toBe("true");
    expect(tones(today)).toEqual(["0", "1"]);
    expect(tones(days()[5]!)).toEqual(["1"]);
    const summary = host.querySelector('[data-testid="mc-board-spend-chart"] [role="img"]')?.getAttribute("aria-label") ?? "";
    expect(summary).toMatch(/120$/);
    expect(summary).not.toContain("10K");

    const filters = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-filter"]')) as HTMLButtonElement[];
    expect(filters.map((filter) => filter.textContent)).toEqual(["All", UNASSIGNED_MODEL_LABEL, "haiku"]);
    await act(async () => filters[2]!.click());
    expect(tones(days()[6]!)).toEqual(["0"]);
    expect(tones(days()[5]!)).toEqual([]);
    await unmount();
  });

  it("mission_control_spend_counts_only_rows_attributed_to_a_team_plan", () => {
    const rows = teamPlanRows(teamSpendFixture());
    expect(rows.map((row) => row.plan_id)).toEqual(["pl_old", "pl_snake", "pl_old"]);
    expect(rows.some((row) => row.uncached_input_tokens === 9_999)).toBe(false);
  });

  it("mission_control_spend_reads_the_team_spend_route_for_a_date_range", async () => {
    const api: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "test-token", cors: false };
    const fixture = teamSpendFixture();
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: async () => fixture });
    vi.stubGlobal("fetch", fetchMock);
    try {
      await expect(
        fetchTeamSpend(api, "team 1", { metric: "tokens", from: "2026-09-07", to: "2026-09-13" }),
      ).resolves.toBe(fixture);
      expect(fetchMock).toHaveBeenCalledWith(
        `${baseUrl(api)}/api/teams/team%201/mission-control/spend?metric=tokens&from=2026-09-07&to=2026-09-13`,
        { headers: authHeaders(api), signal: undefined },
      );
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("mission_control_spend_draws_no_chart_without_allowance_history", async () => {
    const { host, unmount } = await mount(<SpendCard spend={recordingSpend()} />);

    expect(host.querySelector('[data-testid="mc-board-spend-chart"]')).toBeNull();
    const source = allowanceChartSource({}, spendReading(recordingSpend()).groups);
    expect(source.hasData).toBe(false);
    expect(source.metrics).toEqual({ points: false, tokens: false });
    expect(source.days("tokens", "all")).toEqual([]);
    await unmount();
  });

  it("mission_control_spend_reads_every_observed_day_although_each_snapshot_jitters_its_reset_time", () => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const todayStart = today.getTime();
    const resetsAt = todayStart + 2 * 24 * H + 20 * H;
    const jitter = [-1_420, 15, 1_057, -880, 640, -35, 1_490, -1_200];
    const rows: UsageSnapshot[] = [3, 2, 1, 0].flatMap((daysAgo, day) =>
      [0, 1].map(
        (sample) =>
          ({
            parsed_ok: true,
            captured_at: todayStart - daysAgo * 24 * H + (10 + sample) * H,
            weekly_pct: 10 * day + 5 * sample,
            weekly_resets_at: resetsAt + jitter[day * 2 + sample]!,
          }) as unknown as UsageSnapshot,
      ),
    );

    const days = allowanceWeekDays(rows);
    expect(days.length).toBeGreaterThanOrEqual(7);
    const observed = days.filter((day) => day.start >= todayStart - 3 * 24 * H && day.start <= todayStart);
    expect(observed.map((day) => day.pct)).toEqual([5, 5, 5, 5]);
    expect(days.find((day) => day.today)?.pct).toBe(5);
    expect(days.filter((day) => day.start > todayStart).every((day) => day.pct === null)).toBe(true);
    expect(days.every((day) => /^[A-Z][a-z]{2}$/.test(day.label))).toBe(true);
    expect(allowanceWeekDays([])).toEqual([]);
  });
});

describe("mission control models card", () => {
  it("mission_control_models_ranks_every_model_by_new_input_with_one_share_column", async () => {
    const { host, unmount } = await mount(<ModelsCard spend={recordingSpend()} />);

    const headers = Array.from(host.querySelectorAll('thead th[scope="col"]')).map((th) => th.textContent);
    expect(headers).toEqual(["model", "new input", "share"]);
    expect(modelCells(host)).toEqual([
      ["haiku", "100", "54%"],
      [UNASSIGNED_MODEL_LABEL, "90", "40%"],
      ["opus", "20", "6%"],
    ]);
    const unassigned = host.querySelector('[data-unassigned="true"]') as HTMLElement;
    expect(unassigned.querySelector("th")?.getAttribute("scope")).toBe("row");
    for (const row of Array.from(host.querySelectorAll('[data-testid="mc-board-spend-model"]'))) {
      const percents = Array.from(row.querySelectorAll("*")).filter(
        (el) => el.children.length === 0 && /%$/.test(el.textContent?.trim() ?? ""),
      );
      expect(percents).toEqual([row.children[2]]);
    }
    expect(text(host, "mc-board-models-meta")).toBe(SPEND_SCOPE_CAPTION);
    await unmount();
  });

  it("mission_control_models_caps_the_long_tail_behind_show_all", async () => {
    const spend = recordingSpend();
    spend.providers[0]!.models = Array.from({ length: 7 }, (_, index) => ({
      model: `model-${index}`,
      turns: 1,
      input_tokens: 100 * (index + 1),
      uncached_input_tokens: 1_000 * (index + 1),
      output_tokens: 1,
      token_share_pct: index,
    }));
    const { host, unmount } = await mount(<ModelsCard spend={spend} />);

    const rows = () => host.querySelectorAll('[data-testid="mc-board-spend-model"]');
    expect(rows()).toHaveLength(MODELS_PREVIEW);
    expect(modelCells(host)[0]?.[0]).toBe("model-6");
    const more = host.querySelector('[data-testid="mc-board-models-more"]') as HTMLButtonElement;
    expect(more.textContent).toBe("Show all 8");
    await act(async () => more.click());
    expect(rows()).toHaveLength(8);
    await unmount();
  });

  it("mission_control_models_ranks_the_team_models_from_team_plan_rows_only", async () => {
    const { host, unmount } = await mount(<ModelsCard spend={recordingSpend()} teamSpend={teamSpendFixture()} />);

    expect(modelCells(host)).toEqual([
      ["haiku", "100", "59%"],
      [UNASSIGNED_MODEL_LABEL, "70", "41%"],
    ]);
    expect(text(host, "mc-board-models-meta")).toBe("team");
    await unmount();
  });

  it("mission_control_models_says_why_there_is_nothing_to_rank", async () => {
    const { host, unmount } = await mount(<ModelsCard spend={notRecordingSpend("disabled", "kpi.enabled")} />);

    expect(text(host, "mc-board-models-empty")).toBe("not recording, enable kpi.enabled");
    expect(host.querySelector("table")).toBeNull();
    await unmount();
  });
});
