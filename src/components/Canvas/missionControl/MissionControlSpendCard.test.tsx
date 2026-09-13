import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { SpendCard } from "./BoardCards";
import {
  SPEND_ALLOWANCE_SPENT_NEEDS,
  SPEND_SCOPE_CAPTION,
  SPEND_TOOL_CALLS_NEEDS,
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
            turns: 1,
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

async function render(spend: MissionControlSpend) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(<SpendCard spend={spend} />));
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

/** Figure key to its number, read from the bold value beside each plain word. */
function figures(el: Element): Record<string, string> {
  return Object.fromEntries(
    Array.from(el.querySelectorAll("[data-figure]")).map((figure) => [
      figure.getAttribute("data-figure"),
      figure.querySelector("b")?.textContent ?? "",
    ]),
  );
}

function lines(host: HTMLElement, testId: string): HTMLElement[] {
  return Array.from(host.querySelectorAll(`[data-testid="${testId}"]`)) as HTMLElement[];
}

function modelRows(host: HTMLElement) {
  return lines(host, "mc-board-spend-model").map((row) => ({
    row,
    label: row.querySelector(".mc-board-spend-model-name")?.textContent?.trim() ?? "",
    share: row.querySelector(".mc-board-spend-model-share")?.textContent?.trim() ?? "",
    unassigned: row.getAttribute("data-unassigned") === "true",
  }));
}

describe("mission control spend card", () => {
  it("mission_control_spend_tells_allowance_then_took_part_then_by_model", async () => {
    const { host, unmount } = await render(recordingSpend());

    expect(Array.from(host.querySelectorAll("h3")).map((label) => label.textContent)).toEqual([
      "Weekly allowance",
      "Took part",
      "By model",
    ]);
    for (const step of ["mc-board-spend-allowance-provider", "mc-board-spend-took-part-provider"]) {
      expect(
        lines(host, step).map((line) => [line.getAttribute("data-provider"), line.querySelector("h4")?.textContent]),
      ).toEqual([
        ["claude", "Claude"],
        ["codex", "Codex"],
      ]);
    }
    await unmount();
  });

  it("mission_control_spend_reads_tokens_in_plain_words_with_cache_as_input_minus_uncached", async () => {
    const { host, unmount } = await render(recordingSpend());

    const [claude, codex] = lines(host, "mc-board-spend-took-part-provider");
    expect(figures(claude!)).toEqual({ newInput: "120", fromCache: "430", generated: "50", turns: "20" });
    expect(figures(codex!)).toEqual({ newInput: "90", fromCache: "210", generated: "100", turns: "10" });
    expect(
      Array.from(claude!.querySelectorAll("[data-figure] .mc-board-spend-word")).map((word) => word.textContent),
    ).toEqual(["new input", "read from cache", "generated", "turns"]);
    await unmount();
  });

  it("mission_control_spend_lists_every_model_with_one_share_column", async () => {
    const { host, unmount } = await render(recordingSpend());

    const rows = modelRows(host);
    expect(rows.map((row) => [row.label, row.share, row.unassigned])).toEqual([
      ["haiku", "54%", false],
      ["opus", "6%", false],
      [UNASSIGNED_MODEL_LABEL, "40%", true],
    ]);
    expect(figures(rows[0]!.row)).toEqual({ newInput: "100", fromCache: "400", generated: "40", turns: "18" });
    expect(rows[1]!.row.querySelector('[data-figure="turns"] .mc-board-spend-word')?.textContent).toBe("turn");
    for (const { row } of rows) {
      const percents = Array.from(row.querySelectorAll("*")).filter(
        (el) => el.children.length === 0 && /%$/.test(el.textContent?.trim() ?? ""),
      );
      expect(percents).toEqual([row.querySelector(".mc-board-spend-model-share")]);
    }
    await unmount();
  });

  it("mission_control_spend_shows_the_unassigned_model_as_its_own_row", async () => {
    const { host, unmount } = await render(recordingSpend());

    const unassigned = modelRows(host).filter((row) => row.unassigned);
    expect(unassigned).toHaveLength(1);
    expect(unassigned[0]?.label).toBe(UNASSIGNED_MODEL_LABEL);
    expect(figures(unassigned[0]!.row)).toEqual({ newInput: "90", fromCache: "210", generated: "100", turns: "10" });
    expect(unassigned[0]!.row.querySelector(".mc-board-swatch")?.getAttribute("data-unassigned")).toBe("true");
    await unmount();
  });

  it("mission_control_spend_marks_allowance_spent_and_tool_calls_as_needing_data_without_a_number", async () => {
    const { host, unmount } = await render(recordingSpend());

    for (const [step, copy] of [
      ["mc-board-spend-allowance-provider", SPEND_ALLOWANCE_SPENT_NEEDS],
      ["mc-board-spend-took-part-provider", SPEND_TOOL_CALLS_NEEDS],
    ] as const) {
      for (const line of lines(host, step)) {
        const slots = Array.from(line.querySelectorAll('[data-testid="mc-board-spend-needs-data"]'));
        expect(slots.map((slot) => slot.textContent)).toEqual([copy]);
        expect(slots[0]!.textContent).not.toMatch(/\d/);
      }
    }
    await unmount();
  });

  it("mission_control_spend_keeps_the_machine_scope_without_a_plan_total_or_output_headline", async () => {
    const { host, unmount } = await render(recordingSpend());

    const card = host.querySelector('[data-testid="mc-board-spend"]') as HTMLElement;
    expect(Array.from(card.querySelectorAll("h2")).map((heading) => heading.textContent)).toEqual(["Spend"]);
    expect(text(host, "mc-board-spend-scope")).toBe(SPEND_SCOPE_CAPTION);
    expect(card.querySelectorAll("p")).toHaveLength(0);
    const words = (card.textContent ?? "").toLowerCase();
    for (const banned of ["this plan", "output", "total", "bytes", "·"]) {
      expect(words).not.toContain(banned);
    }
    await unmount();
  });

  it("mission_control_spend_never_presents_a_model_share_as_allowance", () => {
    const reading = spendReading(recordingSpend());

    for (const model of reading.models) {
      expect(Object.keys(model)).not.toContain("windows");
    }
    expect(reading.groups.map((group) => group.windows.map((window) => window.variant))).toEqual([
      ["weekly", "session"],
      ["weekly"],
    ]);
    expect(reading.models.reduce((sum, model) => sum + model.sharePct, 0)).toBe(100);
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

  it("mission_control_spend_says_no_data_for_a_provider_with_no_measured_rows", async () => {
    const spend = recordingSpend();
    spend.providers = [
      {
        ...spend.providers[0]!,
        turns: 0,
        input_tokens: 0,
        uncached_input_tokens: 0,
        output_tokens: 0,
        models: [],
      },
    ];
    const { host, unmount } = await render(spend);

    expect(text(host, "mc-board-spend-provider-empty")).toBe("no data");
    expect(host.querySelectorAll('[data-testid="mc-board-spend-model"]')).toHaveLength(0);
    expect(host.querySelector('[data-testid="mc-board-spend-by-model"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(2);
    await unmount();
  });

  it("mission_control_spend_keeps_provider_allowance_when_the_ledger_is_unreadable", async () => {
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
    expect(text(host, "mc-board-spend-provider-empty")).toBe("no data");
    await unmount();
  });
});
