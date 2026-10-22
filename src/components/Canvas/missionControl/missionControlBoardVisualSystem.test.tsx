import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, describe, expect, it } from "vitest";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { MissionControlBoard } from "./MissionControlBoard";
import { UNASSIGNED_MODEL_LABEL } from "./missionControlBoardModel";
import type { MissionControlState, PlanBoardState } from "./missionControlState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const H = 3_600_000;
const NOW = 1_700_000_000_000;
const OWNER_WIDTH = 1456;

const observers: Array<(width: number) => void> = [];

class WidthObserver {
  constructor(private readonly cb: ResizeObserverCallback) {
    observers.push((width) => {
      this.cb(
        [{ contentRect: { width } } as unknown as ResizeObserverEntry],
        this as unknown as ResizeObserver,
      );
    });
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}

(globalThis as { ResizeObserver?: unknown }).ResizeObserver = WidthObserver;

beforeEach(() => {
  observers.length = 0;
});

/** The owner's populated machine: a named Claude model, and Codex with an unresolved and a named model. */
function ownerMission(): MissionControlState {
  return {
    held: { held_reviews: [], open_gates: [], oldest_hold_age_ms: null, verdicts_on_held: 0 },
    bounces: { total_attempts: 2, tasks_on_second_attempt: 1, first_try_done: 0, labelled_bounced: 0 },
    event_log: { recorded_events: 0, truncated: false, entries: [] },
    areas: {
      state: "no_facet",
      facet_key: null,
      facet_label: null,
      groups: [],
      untagged_task_count: 0,
      role_fallback: [
        { executor_role: "accessible-interaction-engineer", task_count: 1, task_ids: ["t1"] },
        { executor_role: "frontend-integration-engineer", task_count: 1, task_ids: ["t2"] },
      ],
    },
    kpi: { state: "not_recording", not_recording_reason: "no_rows_for_plan", enable_setting: null, spend: null },
    spend: {
      scope: "machine",
      state: "recording",
      not_recording_reason: null,
      enable_setting: null,
      measured_tokens: 3_867_500,
      turns: 3751,
      providers: [
        {
          provider: "claude",
          label: "Claude Code",
          allowance: {
            captured_at: NOW,
            session_used_pct: 17,
            session_resets_at: NOW + 2 * H,
            weekly_used_pct: 64,
            weekly_resets_at: NOW + 30 * H,
            stale_reason: null,
          },
          turns: 1973,
          input_tokens: 184_000_000,
          uncached_input_tokens: 1_400_000,
          output_tokens: 195_000,
          models: [
            {
              model: "claude-haiku-4-5",
              turns: 1973,
              input_tokens: 184_000_000,
              uncached_input_tokens: 1_400_000,
              output_tokens: 195_000,
              token_share_pct: 36,
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
            weekly_used_pct: 6,
            weekly_resets_at: NOW + 150 * H,
            stale_reason: null,
          },
          turns: 1778,
          input_tokens: 119_077_500,
          uncached_input_tokens: 2_467_500,
          output_tokens: 182_005,
          models: [
            {
              model: null,
              turns: 1777,
              input_tokens: 119_000_000,
              uncached_input_tokens: 2_400_000,
              output_tokens: 182_000,
              token_share_pct: 62,
            },
            {
              model: "gpt-5.6-luna",
              turns: 1,
              input_tokens: 77_500,
              uncached_input_tokens: 67_500,
              output_tokens: 5,
              token_share_pct: 1.7,
            },
          ],
        },
      ],
    },
  };
}

function ownerBoardState(): PlanBoardState {
  return {
    plan_id: FIXTURE_PLAN.id,
    task_counts: { pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 4, done: 0, cancelled: 0, total: 4 },
    mission_control: ownerMission(),
  };
}

async function renderAt(width: number) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () =>
    root.render(<MissionControlBoard plan={FIXTURE_PLAN} state={ownerBoardState()} now={NOW} />),
  );
  await act(async () => {
    for (const report of observers) report(width);
  });
  const card = (testId: string) => host.querySelector(`[data-testid="${testId}"]`) as HTMLElement;
  return {
    host,
    models: card("mc-board-models"),
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function modelRows(models: HTMLElement): HTMLElement[] {
  return Array.from(models.querySelectorAll('[data-testid="mc-board-spend-model"]')) as HTMLElement[];
}

describe("mission control board visual system", () => {
  it("models retain unassigned identities, compact counts, and fractional shares", async () => {
    const { models, unmount } = await renderAt(OWNER_WIDTH);

    const rows = modelRows(models);
    expect(rows.map((row) => row.querySelector("th")?.textContent)).toEqual([UNASSIGNED_MODEL_LABEL, "haiku", "5.6-luna"]);
    expect(rows.map((row) => row.getAttribute("data-unassigned"))).toEqual(["true", "false", "false"]);
    expect(rows.map((row) => row.children[2]?.textContent)).toEqual(["62%", "36%", "1.7%"]);
    for (const row of rows) {
      const share = row.children[2]?.textContent ?? "";
      expect((row.textContent ?? "").split(share).length - 1).toBe(1);
    }
    expect(rows.map((row) => row.children[1]?.textContent)).toEqual(["2.4M", "1.4M", "68k"]);
    const words = (models.textContent ?? "").toLowerCase();
    for (const banned of ["output", "total", "bytes", "·", "needs"]) {
      expect(words).not.toContain(banned);
    }
    await unmount();
  });

  it("mission_control_board_contains_every_card_in_a_labelled_section", async () => {
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    for (const testId of ["mc-board-spend", "mc-board-models", "mc-board-plan", "mc-board-event-log"]) {
      const panel = host.querySelector(`[data-testid="${testId}"]`) as HTMLElement;
      expect(panel.tagName).toBe("SECTION");
      const labelId = panel.getAttribute("aria-labelledby");
      expect(labelId).toBeTruthy();
      expect(document.getElementById(labelId ?? "")?.textContent?.trim()).toBeTruthy();
    }
    const plan = host.querySelector('[data-testid="mc-board-plan"]') as HTMLElement;
    expect(plan.querySelector('[data-testid="mc-board-plan-heading"]')?.textContent).toBe(FIXTURE_PLAN.title);
    expect(plan.querySelector('[data-testid="mc-board-task-statuses"] [role="img"]')?.getAttribute("aria-label")).toMatch(/^4 tasks/);
    expect(plan.querySelector('[data-testid="mc-board-progress-stats"]')?.textContent).toBe(
      "done0/4just startedblocked0of 4 openfirst try0/21 bounced",
    );
    await unmount();
  });
});
