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
const PHONE_WIDTH = 390;

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

/** A fresh mount per width: happy-dom resolves a node's cascade once, on first read. */
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
    spend: card("mc-board-spend"),
    models: card("mc-board-models"),
    plan: card("mc-board-plan"),
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function textLeaves(root: Element): HTMLElement[] {
  return (Array.from(root.querySelectorAll("*")) as HTMLElement[]).filter(
    (el) => el.children.length === 0 && (el.textContent ?? "").trim().length > 0,
  );
}

function modelRows(models: HTMLElement): HTMLElement[] {
  return Array.from(models.querySelectorAll('[data-testid="mc-board-spend-model"]')) as HTMLElement[];
}

describe("mission control board visual system", () => {
  for (const width of [OWNER_WIDTH, PHONE_WIDTH]) {
    const layout = width === PHONE_WIDTH ? "narrow" : "wide";

    it(`mission_control_board_puts_spend_models_and_plan_in_one_row_above_needs_you_at_${width}px`, async () => {
      const { host, unmount } = await renderAt(width);
      const board = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
      expect(board.getAttribute("data-board-layout")).toBe(layout);

      const row = host.querySelector('[data-testid="mc-board-row"]') as HTMLElement;
      expect(Array.from(row.children).map((card) => card.getAttribute("data-testid"))).toEqual([
        "mc-board-spend",
        "mc-board-models",
        "mc-board-plan",
      ]);
      expect(board.lastElementChild?.getAttribute("data-testid")).toBe("mc-board-event-log");
      for (const removed of [
        '[data-testid="mc-board-progress"]',
        '[data-testid="mc-board-areas-toggle"]',
        '[data-testid="mc-board-spend-table"]',
      ]) {
        expect(host.querySelector(removed)).toBeNull();
      }
      await unmount();
    });

    it(`mission_control_models_keep_the_accepted_data_contract_at_${width}px`, async () => {
      const { models, unmount } = await renderAt(width);

      const rows = modelRows(models);
      expect(rows.map((row) => row.querySelector("th")?.textContent)).toEqual([UNASSIGNED_MODEL_LABEL, "haiku", "5.6-luna"]);
      expect(rows.map((row) => row.getAttribute("data-unassigned"))).toEqual(["true", "false", "false"]);
      expect(rows.map((row) => row.children[2]?.textContent)).toEqual(["62%", "36%", "1.7%"]);
      for (const row of rows) {
        expect(textLeaves(row).filter((leaf) => /%$/.test(leaf.textContent?.trim() ?? ""))).toEqual([row.children[2]]);
      }
      expect(rows.map((row) => row.children[1]?.textContent)).toEqual(["2.4M", "1.4M", "68k"]);
      const words = (models.textContent ?? "").toLowerCase();
      for (const banned of ["output", "total", "bytes", "·", "needs"]) {
        expect(words).not.toContain(banned);
      }
      await unmount();
    });

    it(`mission_control_spend_is_a_team_card_with_no_machine_limits_at_${width}px`, async () => {
      const { spend, unmount } = await renderAt(width);

      expect(spend.querySelector('[data-testid="mc-board-spend-empty"]')?.textContent).toBe("no data");
      const words = (spend.textContent ?? "").toLowerCase();
      for (const banned of ["machine", "limit", "week", "session"]) {
        expect(words).not.toContain(banned);
      }
      expect(spend.querySelector('[data-testid="mc-board-spend-gauge"]')).toBeNull();
      expect(spend.querySelector("table")).toBeNull();
      await unmount();
    });

  }

  it("mission_control_board_contains_every_card_in_a_labelled_section", async () => {
    const { host, unmount } = await renderAt(OWNER_WIDTH);

    for (const testId of ["mc-board-spend", "mc-board-models", "mc-board-plan", "mc-board-event-log"]) {
      const panel = host.querySelector(`[data-testid="${testId}"]`) as HTMLElement;
      expect(panel.tagName).toBe("SECTION");
      const head = panel.firstElementChild as HTMLElement;
      expect(head.tagName).toBe("HEADER");
      expect(panel.getAttribute("aria-labelledby")).toBe(head.querySelector("h2")?.id);
    }
    await unmount();
  });

  it("mission_control_plan_card_reads_by_goal_or_title_then_task_statuses", async () => {
    const { host, plan, unmount } = await renderAt(OWNER_WIDTH);

    expect(plan.querySelector('[data-testid="mc-board-plan-heading"]')?.textContent).toBe(FIXTURE_PLAN.title);
    expect(plan.querySelector('[data-testid="mc-board-tasks-amount"]')?.textContent).toBe("4");
    const statuses = Array.from(plan.querySelectorAll('[data-testid="mc-board-task-statuses"] li')) as HTMLElement[];
    expect(statuses.map((status) => status.getAttribute("data-testid"))).toEqual(["mc-board-task-status-paused"]);
    expect(plan.querySelector('[data-testid="mc-board-progress-stats"]')?.textContent).toBe(
      "done0/4just startedblocked0of 4 openfirst try0/21 bounced",
    );
    expect(host.querySelector('[data-testid="mc-board-needs-you"]')).toBeNull();
    await unmount();
  });
});
