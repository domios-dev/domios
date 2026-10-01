import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import type { ApiConfig } from "../../../lib/types";
import { baseUrl } from "../../MemoryPanel/memoryApi";
import { ModelsCard, SpendCard } from "./BoardCards";
import { SPEND_PERIOD_CAPTION } from "./BoardSpendCard";
import {
  MODELS_PREVIEW,
  SPEND_SCOPE_CAPTION,
  UNASSIGNED_MODEL_LABEL,
  modelSpendRows,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";
import {
  fetchTeamSpend,
  localDayKey,
  teamPlanRows,
  teamPlanShares,
  teamSpendRange,
  teamSpendTotals,
  type TeamSpend,
  type TeamSpendDaily,} from "./teamSpend";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const H = 3_600_000;
const NOW = 1_700_000_000_000;

/** The machine ledger shapes the Models card still falls back to without a team reading. */
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
          { model: "haiku", turns: 18, input_tokens: 500, uncached_input_tokens: 100, output_tokens: 40, token_share_pct: 54 },
          { model: "opus", turns: 2, input_tokens: 50, uncached_input_tokens: 20, output_tokens: 10, token_share_pct: 6 },
        ],
      },
      {
        provider: "codex",
        label: "Codex",
        allowance: null,
        turns: 10,
        input_tokens: 300,
        uncached_input_tokens: 90,
        output_tokens: 100,
        models: [
          { model: null, turns: 10, input_tokens: 300, uncached_input_tokens: 90, output_tokens: 100, token_share_pct: 40 },
        ],
      },
    ],
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

function teamRow(
  day: string,
  planId: string | null,
  planTitle: string | null,
  provider: string,
  model: string | null,
  uncached: number,
): TeamSpendDaily {
  return {
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
  };
}

/** Two team plans on two days, and one plan-less machine row that must never count as team spend. */
function teamSpendFixture(): TeamSpend {
  const today = localDayKey(Date.now());
  const yesterday = localDayKey(Date.now() - 24 * H);
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
      teamRow(yesterday, "pl_old", "Old", "codex", null, 50),
      teamRow(today, "pl_snake", "Snake", "claude", "claude-haiku-4-5", 100),
      teamRow(today, "pl_old", "Old", "codex", null, 20),
      teamRow(today, null, null, "claude", "claude-haiku-4-5", 9_999),
    ],
    weekly_limit_points: [],
    weekly_limit_days: [],
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

function button(host: HTMLElement, testId: string): HTMLButtonElement {
  return host.querySelector(`[data-testid="${testId}"]`) as HTMLButtonElement;
}

function chartDays(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-spend-chart-day"]')) as HTMLElement[];
}

function tones(day: HTMLElement): Array<string | null> {
  return Array.from(day.querySelectorAll('[data-testid="mc-board-spend-chart-segment"]')).map((segment) =>
    segment.getAttribute("data-tone"),
  );
}

function chartSummary(host: HTMLElement): string {
  return host.querySelector('[data-testid="mc-board-spend-chart"] [role="img"]')?.getAttribute("aria-label") ?? "";
}

function modelCells(host: HTMLElement): string[][] {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-spend-model"]')).map((row) =>
    Array.from(row.children).map((cell) => cell.textContent?.trim() ?? ""),
  );
}

describe("mission control spend card", () => {
  it("mission_control_spend_leads_with_the_team_totals_for_the_period", async () => {
    const { host, unmount } = await mount(<SpendCard teamSpend={teamSpendFixture()} />);

    expect(text(host, "mc-board-spend-period")).toBe(SPEND_PERIOD_CAPTION);
    const totals = host.querySelector('[data-testid="mc-board-spend-totals"]') as HTMLElement;
    expect(Array.from(totals.querySelectorAll("dt")).map((term) => term.textContent)).toEqual([
      "new input",
      "generated",
      "turns",
    ]);
    expect(text(host, "mc-board-spend-new-input")).toBe("170");
    expect(text(host, "mc-board-spend-generated")).toBe("3");
    expect(text(host, "mc-board-spend-turns")).toBe("3");
    expect(teamSpendTotals(teamSpendFixture())).toEqual({ newInput: 170, generated: 3, turns: 3 });
    const words = text(host, "mc-board-spend").toLowerCase();
    for (const banned of ["machine", "session", "·"]) {
      expect(words).not.toContain(banned);
    }
    await unmount();
  });

  it("mission_control_spend_stacks_bars_by_model_with_a_legend_of_the_models_present", async () => {
    const { host, unmount } = await mount(<SpendCard teamSpend={teamSpendFixture()} />);

    const breakdown = button(host, "mc-board-spend-breakdown");
    expect(breakdown.textContent).toBe("by model");
    expect(breakdown.getAttribute("aria-pressed")).toBe("false");
    expect(tones(chartDays(host)[6]!)).toEqual(["0"]);
    expect(tones(chartDays(host)[5]!)).toEqual(["0"]);
    expect(host.querySelector('[data-testid="mc-board-spend-legend"]')).toBeNull();
    expect(host.querySelector('[data-testid="mc-board-spend-filter"]')).toBeNull();

    await act(async () => breakdown.click());
    expect(breakdown.getAttribute("aria-pressed")).toBe("true");
    const today = chartDays(host)[6]!;
    expect(today.getAttribute("data-today")).toBe("true");
    expect(tones(today)).toEqual(["0", "1"]);
    expect(tones(chartDays(host)[5]!)).toEqual(["1"]);
    const legend = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-legend-item"]'));
    const modelsPresent = new Set(teamPlanRows(teamSpendFixture()).map((row) => row.model));
    expect(legend).toHaveLength(modelsPresent.size);
    expect(
      legend.map((item) => [item.querySelector(".mc-board-swatch")?.getAttribute("data-tone"), item.textContent]),
    ).toEqual([
      ["0", "haiku"],
      ["1", UNASSIGNED_MODEL_LABEL],
    ]);
    expect(chartSummary(host)).toMatch(/^new input by day: .*120$/);

    await act(async () => breakdown.click());
    expect(host.querySelector('[data-testid="mc-board-spend-legend"]')).toBeNull();
    expect(tones(chartDays(host)[6]!)).toEqual(["0"]);
    await unmount();
  });

  it("mission_control_spend_splits_the_period_by_plan", async () => {
    const { host, unmount } = await mount(<SpendCard teamSpend={teamSpendFixture()} />);

    const rows = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-plan-share"]'));
    expect(
      rows.map((row) => [
        row.querySelector(".mc-board-plan-split-title")?.textContent,
        row.querySelector(".mc-board-plan-split-share")?.textContent,
      ]),
    ).toEqual([
      ["Snake", "59%"],
      ["Old", "41%"],
    ]);
    expect(host.querySelector('[data-testid="mc-board-spend-plan-shares"] .mc-board-swatch')).toBeNull();
    await unmount();

    const single = teamSpendFixture();
    single.daily = single.daily.filter((row) => row.plan_id !== "pl_old");
    expect(teamPlanShares(single)).toHaveLength(1);
    const alone = await mount(<SpendCard teamSpend={single} />);
    expect(alone.host.querySelector('[data-testid="mc-board-spend-plan-shares"]')).toBeNull();
    await alone.unmount();
  });

  it("mission_control_spend_says_no_data_without_a_team_reading", async () => {
    const { host, unmount } = await mount(<SpendCard />);

    expect(text(host, "mc-board-spend-empty")).toBe("no data");
    expect(host.querySelector('[data-testid="mc-board-spend-totals"]')).toBeNull();
    expect(host.querySelector('[data-testid="mc-board-spend-chart"]')).toBeNull();
    expect(host.querySelector('[data-testid="mc-board-spend-period"]')).toBeNull();
    expect(host.querySelector('[data-testid="mc-board-spend-breakdown"]')).toBeNull();
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
        expect.objectContaining({ headers: expect.anything() }),
      );
      await fetchTeamSpend(api, "team 1", { metric: "weekly_limit_points", from: "2026-09-07", to: "2026-09-13" });
      expect(fetchMock).toHaveBeenLastCalledWith(
        `${baseUrl(api)}/api/teams/team%201/mission-control/spend?metric=weekly_limit_points&from=2026-09-07&to=2026-09-13`,
        expect.objectContaining({ headers: expect.anything() }),
      );
    } finally {
      vi.unstubAllGlobals();
    }
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
    for (const row of Array.from(host.querySelectorAll('[data-testid="mc-board-spend-model"]'))) {
      const percents = Array.from(row.querySelectorAll("*")).filter(
        (el) => el.children.length === 0 && /%$/.test(el.textContent?.trim() ?? ""),
      );
      expect(percents).toEqual([row.children[2]]);
    }
    expect(text(host, "mc-board-models-meta")).toBe(SPEND_SCOPE_CAPTION);
    expect(modelSpendRows(recordingSpend()).reduce((sum, row) => sum + row.sharePct, 0)).toBe(100);
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
