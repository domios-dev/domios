import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, describe, expect, it } from "vitest";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { MissionControlBoard } from "./MissionControlBoard";
import {
  BOARD_CARD_COLUMNS,
  BOARD_NARROW_MAX_PX,
  BOARD_RUNWAY_COLUMNS,
  boardLayout,
  formatClock,
} from "./missionControlBoardModel";
import type { MissionControlState, PlanBoardState } from "./missionControlState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const H = 3_600_000;
const DAY = 1_700_000_000_000;
const NOW = DAY + 6 * H;

/** Every observer built by a render, so a test can report a width to the board. */
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

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return {
    host,
    setWidth: async (width: number) => {
      await act(async () => {
        for (const report of observers) report(width);
      });
    },
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function text(host: HTMLElement, testId: string): string {
  return host.querySelector(`[data-testid="${testId}"]`)?.textContent?.trim() ?? "";
}

function areaRows(host: HTMLElement): Array<{ label: string; count: string; share: string }> {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-area-row"]')).map((row) => ({
    label: row.querySelector(".mc-board-kv-key")?.textContent?.trim() ?? "",
    count: row.querySelector(".mc-board-kv-value")?.textContent?.trim() ?? "",
    share: row.querySelector(".mc-board-kv-share")?.textContent?.trim() ?? "",
  }));
}

/** Reads the applied template back however the DOM chose to keep it. */
function gridTemplate(el: HTMLElement): string {
  return el.style.gridTemplateColumns || el.getAttribute("style") || "";
}

function recordingState(): MissionControlState {
  return {
    held: {
      held_reviews: [
        { task_id: "t1", name: "Land the wave", verdicts: 5, last_verdict_at: NOW - H, age_ms: H },
        { task_id: "t2", name: "Triage backlog", verdicts: 3, last_verdict_at: NOW - 600_000, age_ms: 600_000 },
      ],
      open_gates: [
        {
          id: "g1",
          question: "Ship the migration now?",
          status: "open",
          asked_at: NOW - 2 * H,
          age_ms: 2 * H,
          blocking_task_count: 2,
        },
      ],
      oldest_hold_age_ms: H + 420_000,
      verdicts_on_held: 8,
    },
    bounces: {
      total_attempts: 12,
      tasks_on_second_attempt: 1,
      first_try_done: 10,
      labelled_bounced: 2,
    },
    event_log: {
      recorded_events: 81,
      truncated: false,
      entries: [
        {
          ts: DAY + H,
          last_ts: DAY + H,
          count: 1,
          kind: "planning",
          author: "Pasteur",
          author_tab_id: "tab-planner",
          task_id: null,
          task_name: null,
          text: "drafting to approved",
        },
        {
          ts: DAY + H + 60_000,
          last_ts: DAY + H + 60_000,
          count: 1,
          kind: "task_created",
          author: "",
          author_tab_id: null,
          task_id: "t1",
          task_name: "Land the wave",
          text: "Land the wave",
        },
        {
          ts: DAY + 2 * H,
          last_ts: DAY + 2 * H + 900_000,
          count: 4,
          kind: "worker_report",
          author: "Vera",
          author_tab_id: "tab-vera",
          task_id: "t1",
          task_name: "Land the wave",
          text: "In Review",
        },
      ],
    },
    areas: {
      state: "declared",
      facet_key: "area",
      facet_label: "Area",
      groups: [
        { value: "design", label: "Design", task_count: 6, task_ids: ["t1"] },
        { value: "backend", label: "Backend", task_count: 2, task_ids: ["t2"] },
      ],
      untagged_task_count: 2,
      role_fallback: [],
    },
    kpi: {
      state: "recording",
      not_recording_reason: null,
      spend: {
        events: 4023,
        bytes: 65_000_000,
        turns: 4023,
        tokens: { input: 1_400_000_000, uncached_input: 1_500_000, output: 5_300_000, total: 1_405_300_000 },
      },
    },
    runway: {
      state: "reporting",
      providers: [
        {
          provider: "claude",
          label: "Claude",
          captured_at: NOW,
          session_pct: 58,
          session_resets_at: NOW + 2 * H,
          weekly_pct: 34,
          weekly_resets_at: NOW + 48 * H,
          stale_reason: null,
        },
        {
          provider: "codex",
          label: "Codex",
          captured_at: NOW,
          session_pct: null,
          session_resets_at: null,
          weekly_pct: 81,
          weekly_resets_at: NOW + 72 * H,
          stale_reason: null,
        },
      ],
    },
  };
}

function notRecordingState(): MissionControlState {
  const base = recordingState();
  return {
    ...base,
    kpi: { state: "not_recording", not_recording_reason: "disabled", spend: null },
    areas: {
      state: "no_facet",
      facet_key: null,
      facet_label: null,
      groups: [],
      untagged_task_count: 3,
      role_fallback: [
        { executor_role: "frontend-dev", task_count: 2, task_ids: ["t1", "t2"] },
        { executor_role: null, task_count: 1, task_ids: ["t3"] },
      ],
    },
    runway: { state: "not_reporting", providers: [] },
  };
}

function boardState(mission: MissionControlState): PlanBoardState {
  return {
    plan_id: FIXTURE_PLAN.id,
    task_counts: {
      pending: 1,
      in_progress: 0,
      in_review: 2,
      blocked: 0,
      paused: 0,
      done: 10,
      cancelled: 1,
      total: 14,
    },
    mission_control: mission,
  };
}

const PLAN: Plan = { ...FIXTURE_PLAN, milestone_kinds: [{ key: "skill", label: "Skill pinned", color: null }] };

function board(mission: MissionControlState) {
  return <MissionControlBoard plan={PLAN} state={boardState(mission)} now={NOW} />;
}

beforeEach(() => {
  observers.length = 0;
});

describe("mission control board cards", () => {
  it("reads progress and ledger-backed spend into the status header", async () => {
    const { host, unmount } = await render(board(recordingState()));

    expect(host.querySelector('[data-testid="plan-board-status"]')).not.toBeNull();
    expect(text(host, "mc-board-progress")).toBe("10 of 13 done · 2 in review · 1 cancelled");
    expect(text(host, "mc-board-total-spend")).toContain("5.3M");
    expect(text(host, "mc-board-total-spend")).toContain("1.4B");
    await unmount();
  });

  it("reads tokens, turns, bounces and held decisions from the stored figures", async () => {
    const { host, unmount } = await render(board(recordingState()));

    expect(text(host, "mc-board-tokens-amount")).toBe("5.3M");
    expect(text(host, "mc-board-turns-amount")).toBe("4,023");
    expect(text(host, "mc-board-bounces-amount")).toBe("1");
    expect(text(host, "mc-board-bounces")).toContain("12");
    expect(text(host, "mc-board-held-amount")).toBe("2");
    expect(text(host, "mc-board-held")).toContain("1 h 07 m");
    expect(text(host, "mc-board-held")).toContain("8");
    await unmount();
  });

  it("renders one gauge per reported provider cycle with its own reset", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const gauges = host.querySelectorAll('[data-testid="mc-board-runway-gauge"]');
    expect(Array.from(gauges).map((g) => g.getAttribute("data-runway-key"))).toEqual([
      "claude-session",
      "claude-weekly",
      "codex-weekly",
    ]);
    const first = gauges[0] as HTMLElement;
    expect(first.querySelector(".ct-usage-gauge .pct")?.textContent).toBe("58%");
    expect(first.querySelector(".ct-usage-gauge .reset")?.textContent).toContain("resets");
    const warned = gauges[2] as HTMLElement;
    expect(warned.querySelector(".ct-usage-gauge")?.className).toContain("warn");
    await unmount();
  });

  it("groups by the declared area facet and names the untagged remainder", async () => {
    const { host, unmount } = await render(board(recordingState()));

    expect(areaRows(host)).toEqual([
      { label: "Design", count: "6", share: "60%" },
      { label: "Backend", count: "2", share: "20%" },
    ]);
    expect(text(host, "mc-board-area-untagged")).toContain("2");
    expect(host.querySelector('[data-testid="mc-board-area-no-facet"]')).toBeNull();
    await unmount();
  });
});

describe("mission control board fallback states", () => {
  it("states the ledger is not recording instead of showing a figure", async () => {
    const { host, unmount } = await render(board(notRecordingState()));

    expect(text(host, "mc-board-tokens-amount")).toBe("—");
    expect(text(host, "mc-board-turns-amount")).toBe("—");
    expect(text(host, "mc-board-tokens")).toContain("not recording");
    expect(host.querySelector('[data-testid="mc-board-total-spend"]')).toBeNull();
    await unmount();
  });

  it("falls back to stored executor roles with an explicit no-facet message", async () => {
    const { host, unmount } = await render(board(notRecordingState()));

    const rows = areaRows(host);
    expect(rows).toHaveLength(2);
    expect(rows[0]?.label).toContain("frontend-dev");
    expect(rows[0]?.label).toContain("executor role, not a tag");
    expect(rows[0]?.count).toBe("2");
    expect(rows[0]?.share).toBe("67%");
    expect(rows[1]?.label).toContain("No role recorded");
    expect(rows[1]?.share).toBe("33%");
    expect(text(host, "mc-board-area-no-facet")).toContain("declares no area facet");
    await unmount();
  });

  it("says no provider reports usage rather than drawing an empty gauge", async () => {
    const { host, unmount } = await render(board(notRecordingState()));

    expect(host.querySelectorAll('[data-testid="mc-board-runway-gauge"]')).toHaveLength(0);
    expect(text(host, "mc-board-runway-empty")).toContain("No provider reports usage");
    await unmount();
  });
});

describe("mission control board event log", () => {
  it("mission_control_event_log_renders_each_title_once", async () => {
    const title = "Verify clean first run";
    const event = {
      ts: DAY + H,
      last_ts: DAY + H,
      count: 1,
      kind: "task_created",
      author: "Planner",
      author_tab_id: "tab-planner",
      task_id: "verify-clean-first-run",
      task_name: title,
      text: title,
    };
    const mission = recordingState();
    mission.event_log.entries = [event];

    const { host, unmount } = await render(board(mission));

    const row = host.querySelector('[data-testid="mc-board-event-row"]') as HTMLElement;
    const rowText = row.textContent ?? "";
    expect(row.querySelector(".mc-board-log-time")?.textContent).toBe(formatClock(event.ts));
    expect(row.querySelector(".mc-board-log-kind")?.textContent).toBe("Task added");
    expect(row.querySelector('[data-testid="mc-board-event-author"]')?.textContent).toBe("Planner");
    expect(rowText.split(title).length - 1).toBe(1);
    await unmount();
  });

  it("keeps events in time order under hour bands and collapses one agent's report run", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const bands = Array.from(host.querySelectorAll('[data-testid="mc-board-event-hour"]')).map(
      (b) => b.textContent,
    );
    expect(bands).toEqual([
      new Date(DAY + H).getHours().toString().padStart(2, "0") + ":00",
      new Date(DAY + 2 * H).getHours().toString().padStart(2, "0") + ":00",
    ]);

    const rows = Array.from(host.querySelectorAll('[data-testid="mc-board-event-row"]'));
    expect(rows).toHaveLength(3);
    expect(rows[0]?.textContent).toContain("drafting to approved");
    expect(rows[1]?.textContent).toContain("Task added");

    const runs = host.querySelectorAll('[data-testid="mc-board-event-run"]');
    expect(runs).toHaveLength(1);
    expect(runs[0]?.textContent).toMatch(/^×4, \d\d:\d\d to \d\d:\d\d$/);
    await unmount();
  });

  it("takes authors from the plan record and never interprets the note body", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const authors = Array.from(host.querySelectorAll('[data-testid="mc-board-event-author"]')).map(
      (a) => a.textContent,
    );
    expect(authors).toEqual(["Pasteur", "", "Vera"]);
    await unmount();
  });

  it("prefers the kind label the plan itself declares", async () => {
    const skill = recordingState();
    skill.event_log.entries = [
      {
        ts: DAY + H,
        last_ts: DAY + H,
        count: 1,
        kind: "skill",
        author: "Pasteur",
        author_tab_id: "tab-planner",
        task_id: null,
        task_name: null,
        text: "verify-before-claim kept",
      },
    ];
    const { host, unmount } = await render(board(skill));

    expect(host.querySelector('[data-testid="mc-board-event-row"]')?.textContent).toContain("Skill pinned");
    await unmount();
  });
});

describe("mission control board layout", () => {
  it("resolves a 390px viewport to the narrow, zero-minimum grid", () => {
    expect(boardLayout(390)).toBe("narrow");
    expect(boardLayout(BOARD_NARROW_MAX_PX)).toBe("narrow");
    expect(boardLayout(BOARD_NARROW_MAX_PX + 1)).toBe("wide");
    expect(BOARD_CARD_COLUMNS.narrow).toBe("repeat(2, minmax(0, 1fr))");
    expect(BOARD_RUNWAY_COLUMNS.narrow).toBe("minmax(0, 1fr)");
  });

  it("binds every grid track to a zero minimum at 390px so nothing can push the board wider", async () => {
    const { host, setWidth, unmount } = await render(board(recordingState()));
    await setWidth(390);

    const root = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(root.getAttribute("data-board-layout")).toBe("narrow");

    const cards = host.querySelector('[data-testid="mc-board-card-grid"]') as HTMLElement;
    const runway = host.querySelector('[data-testid="mc-board-runway-grid"]') as HTMLElement;
    const tracks = `${gridTemplate(cards)} ${gridTemplate(runway)}`;

    expect(tracks).toContain("repeat(2, minmax(0, 1fr))");
    expect(tracks).toContain("minmax(0, 1fr)");
    expect(tracks).not.toMatch(/minmax\(\s*(?!0)/);
    expect(tracks).not.toMatch(/\d+px/);
    await unmount();
  });

  it("returns to the wide grid above the narrow bound", async () => {
    const { host, setWidth, unmount } = await render(board(recordingState()));
    await setWidth(390);
    await setWidth(1200);

    const root = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(root.getAttribute("data-board-layout")).toBe("wide");
    const cards = host.querySelector('[data-testid="mc-board-card-grid"]') as HTMLElement;
    expect(gridTemplate(cards)).toContain("1.6fr");
    await unmount();
  });

  it("offers keyboard-reachable mutation controls without drag affordances", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const boardRoot = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(boardRoot.querySelector("[draggable]")).toBeNull();
    const trigger = boardRoot.querySelector('[data-testid="mission-control-board-menu-trigger"]') as HTMLButtonElement;
    expect(trigger).not.toBeNull();
    expect(trigger.tabIndex).toBe(0);
    expect(boardRoot.querySelector("input, select, textarea")).toBeNull();
    await unmount();
  });
});
