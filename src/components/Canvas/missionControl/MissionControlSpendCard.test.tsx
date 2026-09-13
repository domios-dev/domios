import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";
import type { UsageSnapshot } from "../../../types/usage";
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
    expect(gauges.map((gauge) => gauge.textContent)).toEqual(["week 34%", "session 58%", "week 81%"]);
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

  it("mission_control_spend_draws_a_provider_week_by_day_from_allowance_history", async () => {
    const pcts = [10, 20, 4, null, null, null, null];
    const days: AllowanceDay[] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map((label, index) => ({
      start: NOW + index * 24 * H,
      label,
      pct: pcts[index] ?? null,
      today: index === 2,
    }));
    const weeks: AllowanceWeeks = { claude: days };
    const { host, unmount } = await mount(<SpendCard spend={recordingSpend()} weeks={weeks} />);

    const found = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-week"]')) as HTMLElement[];
    expect(found).toHaveLength(1);
    const week = found[0]!;
    expect(week.closest('[data-testid="mc-board-spend-provider"]')?.getAttribute("data-provider")).toBe("claude");
    expect(week.getAttribute("role")).toBe("img");
    expect(week.getAttribute("aria-label")).toBe("Claude weekly allowance by day: Mon 10%, Tue 20%, Wed 4%");
    expect(week.textContent).toBe("");
    const bars = Array.from(week.querySelectorAll('[data-testid="mc-board-spend-day"]')) as HTMLElement[];
    expect(bars.map((bar) => bar.style.height)).toEqual(["50%", "100%", "20%", "0%", "0%", "0%", "0%"]);
    expect(bars.map((bar) => bar.getAttribute("data-today"))).toEqual([null, null, "true", null, null, null, null]);
    expect(bars.map((bar) => bar.getAttribute("data-future"))).toEqual([null, null, null, "true", "true", "true", "true"]);
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

  it("mission_control_models_says_why_there_is_nothing_to_rank", async () => {
    const { host, unmount } = await mount(<ModelsCard spend={notRecordingSpend("disabled", "kpi.enabled")} />);

    expect(text(host, "mc-board-models-empty")).toBe("not recording, enable kpi.enabled");
    expect(host.querySelector("table")).toBeNull();
    await unmount();
  });
});
