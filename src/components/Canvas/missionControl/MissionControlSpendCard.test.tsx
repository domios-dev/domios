import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";
import type { WeeklyProjection } from "../../../hooks/useUsageSnapshot";
import type { UsageSnapshot } from "../../../types/usage";
import { allowanceWeekDays, type AllowanceDay, type AllowanceWeeks } from "./allowanceWeek";
import { SpendCard } from "./BoardCards";
import {
  SPEND_SCOPE_CAPTION,
  SPEND_SHARE_COLUMN,
  SPEND_TOKEN_COLUMNS,
  UNASSIGNED_MODEL_LABEL,
  spendReading,
  spendTokens,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";

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

async function render(spend: MissionControlSpend, weeks?: AllowanceWeeks) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(<SpendCard spend={spend} weeks={weeks} />));
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

/** Each cell paired with the column header at its own index, as a screen reader pairs them. */
function cellsByColumn(row: Element): Record<string, string> {
  const table = row.closest("table") as HTMLTableElement;
  const headers = Array.from(table.querySelector("thead tr")?.children ?? []).map(
    (cell) => cell.getAttribute("aria-label") ?? "",
  );
  return Object.fromEntries(
    Array.from(row.children)
      .map((cell, index) => [headers[index] ?? "", cell.textContent?.trim() ?? ""] as const)
      .filter(([header]) => header !== ""),
  );
}

function groups(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-spend-provider"]')) as HTMLElement[];
}

function modelRows(root: Element): HTMLElement[] {
  return Array.from(root.querySelectorAll('[data-testid="mc-board-spend-model"]')) as HTMLElement[];
}

describe("mission control spend card", () => {
  it("mission_control_spend_names_each_provider_once_with_its_models_under_it", async () => {
    const { host, unmount } = await render(recordingSpend());

    expect(groups(host).map((group) => group.getAttribute("data-provider"))).toEqual(["claude", "codex"]);
    const card = text(host, "mc-board-spend");
    for (const label of ["Claude", "Codex"]) {
      expect(card.split(label)).toHaveLength(2);
    }
    const names = (group: HTMLElement) => modelRows(group).map((row) => row.querySelector("th")?.textContent);
    expect(names(groups(host)[0]!)).toEqual(["haiku", "opus"]);
    expect(names(groups(host)[1]!)).toEqual([UNASSIGNED_MODEL_LABEL]);
    await unmount();
  });

  it("mission_control_spend_names_each_column_once_with_short_names_for_a_narrow_panel", async () => {
    const { host, unmount } = await render(recordingSpend());

    const headers = Array.from(host.querySelectorAll('thead th[scope="col"]'));
    const columns = [SPEND_SHARE_COLUMN, ...SPEND_TOKEN_COLUMNS];
    expect(headers.map((th) => th.getAttribute("aria-label"))).toEqual(columns.map((column) => column.label));
    expect(headers.map((th) => th.querySelector(".mc-board-spend-label-short")?.textContent)).toEqual(
      columns.map((column) => column.short),
    );
    for (const row of Array.from(host.querySelectorAll("tbody tr"))) {
      const words = (row.textContent ?? "").toLowerCase();
      for (const column of SPEND_TOKEN_COLUMNS) {
        expect(words).not.toContain(column.label);
      }
    }
    await unmount();
  });

  it("mission_control_spend_reads_figures_under_their_columns_with_cached_as_input_minus_uncached", async () => {
    const { host, unmount } = await render(recordingSpend());

    const [claude, codex] = groups(host);
    expect(cellsByColumn(claude!.querySelector('[data-testid="mc-board-spend-group"]')!)).toEqual({
      share: "60%",
      "new input": "120",
      cached: "430",
      generated: "50",
      turns: "20",
    });
    expect(cellsByColumn(modelRows(claude!)[0]!)).toEqual({
      share: "54%",
      "new input": "100",
      cached: "400",
      generated: "40",
      turns: "18",
    });
    expect(cellsByColumn(codex!.querySelector('[data-testid="mc-board-spend-group"]')!)).toEqual({
      share: "40%",
      "new input": "90",
      cached: "210",
      generated: "100",
      turns: "10",
    });
    await unmount();
  });

  it("mission_control_spend_shows_the_unassigned_model_as_its_own_row", async () => {
    const { host, unmount } = await render(recordingSpend());

    const unassigned = modelRows(host).filter((row) => row.getAttribute("data-unassigned") === "true");
    expect(unassigned).toHaveLength(1);
    expect(unassigned[0]!.querySelector("th")?.textContent).toBe(UNASSIGNED_MODEL_LABEL);
    expect(cellsByColumn(unassigned[0]!)).toEqual({
      share: "40%",
      "new input": "90",
      cached: "210",
      generated: "100",
      turns: "10",
    });
    await unmount();
  });

  it("mission_control_spend_reads_allowance_as_week_and_session_on_the_provider_row", async () => {
    const { host, unmount } = await render(recordingSpend());

    const gauges = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')) as HTMLElement[];
    expect(gauges.map((gauge) => gauge.textContent)).toEqual(["week 34%", "session 58%", "week 81%"]);
    for (const gauge of gauges) {
      expect(gauge.closest("tr")?.getAttribute("data-testid")).toBe("mc-board-spend-group");
    }
    expect(gauges[2]!.querySelector("b")?.classList.contains("mc-board-warn")).toBe(true);
    for (const row of modelRows(host)) {
      expect(Array.from(row.querySelectorAll("td")).filter((cell) => /%$/.test(cell.textContent ?? ""))).toEqual([
        row.children[1],
      ]);
    }
    await unmount();
  });

  it("mission_control_spend_keeps_the_machine_scope_with_no_prose_or_placeholder", async () => {
    const { host, unmount } = await render(recordingSpend());

    const card = host.querySelector('[data-testid="mc-board-spend"]') as HTMLElement;
    expect(Array.from(card.querySelectorAll("h2")).map((heading) => heading.textContent)).toEqual(["Spend"]);
    expect(text(host, "mc-board-spend-scope")).toBe(SPEND_SCOPE_CAPTION);
    expect(card.querySelectorAll("p")).toHaveLength(0);
    const words = (card.textContent ?? "").toLowerCase();
    for (const banned of ["this plan", "output", "total", "bytes", "·", "needs", "spent", "used", "took part"]) {
      expect(words).not.toContain(banned);
    }
    await unmount();
  });

  it("mission_control_spend_never_presents_a_model_share_as_allowance", () => {
    const reading = spendReading(recordingSpend());

    for (const group of reading.groups) {
      for (const model of group.models) {
        expect(Object.keys(model)).not.toContain("windows");
      }
    }
    expect(reading.groups.map((group) => group.windows.map((window) => window.variant))).toEqual([
      ["weekly", "session"],
      ["weekly"],
    ]);
    expect(reading.groups.map((group) => group.sharePct)).toEqual([60, 40]);
    expect(spendTokens({ turns: 1, input_tokens: 5, uncached_input_tokens: 9, output_tokens: 0 }).fromCache).toBe(0);
  });

  it("mission_control_spend_reports_each_not_recording_state_without_a_dash", async () => {
    for (const [spend, copy] of [
      [notRecordingSpend("disabled", "kpi.enabled"), "not recording, enable kpi.enabled"],
      [notRecordingSpend("unreadable"), "ledger unavailable"],
      [notRecordingSpend("no_rows_for_plan"), "no data"],
    ] as const) {
      const { host, unmount } = await render(spend);
      expect(text(host, "mc-board-spend-empty")).toBe(copy);
      expect(text(host, "mc-board-spend")).not.toContain("—");
      expect(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(0);
      await unmount();
    }
  });

  it("mission_control_spend_leaves_a_provider_without_measured_rows_blank_but_keeps_its_allowance", async () => {
    const measured = recordingSpend();
    const spend: MissionControlSpend = {
      ...notRecordingSpend("unreadable"),
      providers: [
        {
          ...measured.providers[0]!,
          turns: 0,
          input_tokens: 0,
          uncached_input_tokens: 0,
          output_tokens: 0,
          models: [],
        },
      ],
    };
    const { host, unmount } = await render(spend);

    expect(text(host, "mc-board-spend-units")).toBe("ledger unavailable");
    expect(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(2);
    expect(modelRows(host)).toHaveLength(0);
    const cells = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-group"] td'));
    expect(cells.map((cell) => cell.textContent)).toEqual(["", "", "", "", ""]);
    await unmount();
  });

  it("mission_control_spend_draws_a_provider_week_by_day_from_allowance_history", async () => {
    const pcts = [10, 20, 4, null, null, null, null];
    const days: AllowanceDay[] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map((label, index) => ({
      start: NOW + index * 24 * H,
      label,
      pct: pcts[index] ?? null,
      today: index === 2,
    }));
    const { host, unmount } = await render(recordingSpend(), { claude: days });

    const weeks = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-week"]')) as HTMLElement[];
    expect(weeks).toHaveLength(1);
    const week = weeks[0]!;
    expect(week.closest("tbody")?.getAttribute("data-provider")).toBe("claude");
    expect(week.closest("tr")?.getAttribute("data-testid")).toBe("mc-board-spend-group");
    expect(week.getAttribute("role")).toBe("img");
    expect(week.getAttribute("aria-label")).toBe("Claude weekly allowance by day: Mon 10%, Tue 20%, Wed 4%");
    expect(week.textContent).toBe("");
    const bars = Array.from(week.querySelectorAll('[data-testid="mc-board-spend-day"]')) as HTMLElement[];
    expect(bars.map((bar) => bar.style.height)).toEqual(["50%", "100%", "20%", "0%", "0%", "0%", "0%"]);
    expect(bars.map((bar) => bar.getAttribute("data-today"))).toEqual([null, null, "true", null, null, null, null]);
    expect(bars.map((bar) => bar.getAttribute("data-future"))).toEqual([null, null, null, "true", "true", "true", "true"]);
    await unmount();
  });

  it("mission_control_spend_reads_the_week_from_day_actuals_and_leaves_days_ahead_empty", () => {
    const now = Date.now();
    const today = new Date(now);
    today.setHours(0, 0, 0, 0);
    const rows = [
      { captured_at: now, weekly_pct: 30, weekly_resets_at: today.getTime() + 3 * 24 * H } as unknown as UsageSnapshot,
    ];
    const weekly = {
      avg_weekday_spend: 10,
      avg_weekend_spend: 5,
      dow_estimates: [],
      rest_of_today_est: 2,
      day_actuals: [{ day_start_ms: today.getTime(), pct: 7 }],
    } as unknown as WeeklyProjection;

    const days = allowanceWeekDays(rows, weekly);
    expect(days.length).toBeGreaterThanOrEqual(7);
    expect(days.find((day) => day.today)?.pct).toBe(7);
    expect(days.filter((day) => day.start > today.getTime()).every((day) => day.pct === null)).toBe(true);
    expect(allowanceWeekDays([], null)).toEqual([]);
  });
});
