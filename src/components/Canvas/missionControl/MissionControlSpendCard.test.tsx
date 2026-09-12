import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { SpendCard } from "./BoardCards";
import {
  BOARD_SPEND_WINDOW_COLUMNS,
  SPEND_SCOPE_CAPTION,
  UNASSIGNED_MODEL_LABEL,
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

async function render(spend: MissionControlSpend, layout: "wide" | "narrow" = "wide") {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(<SpendCard spend={spend} layout={layout} now={NOW} />));
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

/** Each figure is labelled by the column header at its own cell index, as a screen reader pairs them. */
function modelRows(host: HTMLElement) {
  const table = host.closest("table") ?? host.querySelector("table");
  const headers = Array.from(table?.querySelectorAll('thead [role="columnheader"]') ?? []).map(
    (th) => th.textContent?.trim() ?? "",
  );
  return Array.from(host.querySelectorAll('[data-testid="mc-board-spend-model"]')).map((row) => {
    const cells = Array.from(row.children);
    return {
      label: row.querySelector(".mc-board-spend-model-name")?.textContent?.trim() ?? "",
      share: row.querySelector(".mc-board-spend-model-share")?.textContent?.trim() ?? "",
      unassigned: row.getAttribute("data-unassigned") === "true",
      figures: Array.from(row.querySelectorAll(".mc-board-spend-figure")).map((figure) => ({
        label: headers[cells.indexOf(figure)] ?? "",
        value: figure.textContent?.trim() ?? "",
      })),
    };
  });
}

describe("mission control spend card", () => {
  it("mission_control_spend_groups_model_rows_under_their_provider", async () => {
    const { host, unmount } = await render(recordingSpend());

    const providers = Array.from(
      host.querySelectorAll('[data-testid="mc-board-spend-provider"]'),
    ).map((group) => group.getAttribute("data-provider"));
    expect(providers).toEqual(["claude", "codex"]);

    const claude = host.querySelector('[data-provider="claude"]') as HTMLElement;
    expect(modelRows(claude).map((row) => row.label)).toEqual(["haiku", "opus"]);
    expect(modelRows(claude)[0]?.figures).toEqual([
      { label: "turns", value: "18" },
      { label: "input", value: "500" },
      { label: "uncached", value: "100" },
      { label: "output", value: "40" },
    ]);
    expect(modelRows(claude)[0]?.share).toBe("54%");
    await unmount();
  });

  it("mission_control_spend_shows_the_unassigned_model_as_its_own_row", async () => {
    const { host, unmount } = await render(recordingSpend());

    const unassigned = modelRows(host).filter((row) => row.unassigned);
    expect(unassigned).toHaveLength(1);
    expect(unassigned[0]?.label).toBe(UNASSIGNED_MODEL_LABEL);
    expect(unassigned[0]?.share).toBe("40%");
    expect(unassigned[0]?.figures.map((figure) => figure.value)).toEqual(["10", "300", "90", "100"]);
    await unmount();
  });

  it("mission_control_spend_keeps_a_heading_and_the_figures_without_prose", async () => {
    const { host, unmount } = await render(recordingSpend());

    const card = host.querySelector('[data-testid="mc-board-spend"]') as HTMLElement;
    expect(card.querySelectorAll("h2")).toHaveLength(1);
    expect(card.querySelector("h2")?.textContent).toBe("Spend");
    expect(text(host, "mc-board-spend-scope")).toBe(SPEND_SCOPE_CAPTION);
    expect(card.querySelectorAll("p")).toHaveLength(0);
    expect(host.querySelector('[data-testid="mc-board-spend-units"]')).toBeNull();
    await unmount();
  });

  it("mission_control_spend_names_each_allowance_window_once_under_its_provider", async () => {
    const { host, unmount } = await render(recordingSpend());

    const gauges = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]'));
    expect(gauges.map((gauge) => gauge.querySelector(".label")?.textContent)).toEqual([
      "session",
      "weekly",
      "session",
      "weekly",
    ]);
    expect(gauges.map((gauge) => gauge.getAttribute("aria-label"))).toEqual([
      "Claude session allowance",
      "Claude weekly allowance",
      "Codex session allowance",
      "Codex weekly allowance",
    ]);
    expect(gauges.map((gauge) => gauge.getAttribute("data-reading"))).toEqual([
      "present",
      "present",
      "none",
      "present",
    ]);
    for (const group of Array.from(host.querySelectorAll('[data-testid="mc-board-spend-provider"]'))) {
      const name = group.querySelector('th[scope="rowgroup"]')?.textContent ?? "";
      expect(group.textContent?.split(name)).toHaveLength(2);
    }
    await unmount();
  });

  it("mission_control_spend_names_the_machine_scope_and_never_a_plan_total", async () => {
    const spend = recordingSpend();
    expect(spend.scope).toBe("machine");
    const { host, unmount } = await render(spend);

    const card = text(host, "mc-board-spend");
    expect(card).toContain(SPEND_SCOPE_CAPTION);
    expect(card.toLowerCase()).not.toContain("this plan");
    await unmount();
  });

  it("mission_control_spend_never_presents_a_model_share_as_allowance", async () => {
    const reading = spendReading(recordingSpend());

    for (const group of reading.groups) {
      for (const model of group.models) {
        expect(model.figures.map((figure) => figure.label)).toEqual([
          "turns",
          "input",
          "uncached",
          "output",
        ]);
      }
      expect(group.windows.map((gauge) => gauge.variant)).toEqual(["session", "weekly"]);
    }
    const shares = reading.groups.flatMap((group) => group.models.map((model) => model.sharePct));
    expect(shares.reduce((sum, share) => sum + share, 0)).toBe(100);
  });

  it("mission_control_spend_reports_each_not_recording_state_without_a_dash", async () => {
    for (const [spend, copy] of [
      [notRecordingSpend("disabled", "kpi.enabled"), "not recording · enable kpi.enabled"],
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

  it("mission_control_spend_stacks_the_allowance_windows_at_the_narrow_layout", async () => {
    const wide = await render(recordingSpend(), "wide");
    const wideGrid = wide.host.querySelector(
      '[data-testid="mc-board-spend-window-grid"]',
    ) as HTMLElement;
    expect(wideGrid.style.gridTemplateColumns).toBe(BOARD_SPEND_WINDOW_COLUMNS.wide);
    await wide.unmount();

    const narrow = await render(recordingSpend(), "narrow");
    const grids = Array.from(
      narrow.host.querySelectorAll('[data-testid="mc-board-spend-window-grid"]'),
    ) as HTMLElement[];
    expect(grids).toHaveLength(2);
    for (const grid of grids) {
      expect(grid.style.gridTemplateColumns).toBe(BOARD_SPEND_WINDOW_COLUMNS.narrow);
      expect(grid.style.gridTemplateColumns).not.toMatch(/\d+px/);
      expect(grid.style.gridTemplateColumns).not.toMatch(/minmax\(\s*(?!0)/);
    }
    // Every label and figure still renders at phone width; nothing is clipped away.
    expect(modelRows(narrow.host)).toHaveLength(3);
    expect(text(narrow.host, "mc-board-spend")).toContain(UNASSIGNED_MODEL_LABEL);
    expect(narrow.host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(4);
    await narrow.unmount();
  });
});
