import { act, type ReactElement } from "react";
import { readFileSync } from "node:fs";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, describe, expect, it } from "vitest";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { MissionControlBoard } from "./MissionControlBoard";
import {
  BOARD_NARROW_MAX_PX,
  BOARD_ROW_COLUMNS,
  EVENT_PART_SEPARATOR,
  EVENT_PREVIEW_MAX,
  EVENT_REVEAL_HIDE_LABEL,
  EVENT_REVEAL_SHOW_LABEL,
  EVENT_TASK_NAME_MAX,
  boardLayout,
  formatClock,
} from "./missionControlBoardModel";
import type {
  KpiNotRecordingReason,
  MissionControlState,
  PlanBoardState,
  PlanEvent,
  PlanTaskCounts,
} from "./missionControlState";

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

function areaRows(host: HTMLElement): Array<{ label: string; count: string }> {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-area-row"]')).map((row) => ({
    label: row.querySelector(".mc-board-kv-key")?.textContent?.trim() ?? "",
    count: row.querySelector(".mc-board-kv-value")?.textContent?.trim() ?? "",
  }));
}

/** Reads the applied template back however the DOM chose to keep it. */
function gridTemplate(el: HTMLElement): string {
  return el.style.gridTemplateColumns || el.getAttribute("style") || "";
}

async function openAreas(host: HTMLElement): Promise<HTMLButtonElement> {
  const toggle = host.querySelector('[data-testid="mc-board-areas-toggle"]') as HTMLButtonElement;
  await act(async () => toggle.click());
  return toggle;
}

function statusItems(host: HTMLElement): string[] {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-task-statuses"] li')).map(
    (item) => item.textContent ?? "",
  );
}

function recordingState(): MissionControlState {
  return {
    held: {
      held_reviews: [
        {
          task_id: "t1",
          name: "Land the wave",
          verdicts: 5,
          last_verdict_at: NOW - (H + 420_000),
          age_ms: H + 420_000,
        },
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
      enable_setting: null,
      spend: {
        events: 4023,
        bytes: 65_000_000,
        turns: 4023,
        tokens: { input: 1_400_000_000, uncached_input: 1_500_000, output: 5_300_000, total: 1_405_300_000 },
      },
    },
    spend: {
      scope: "machine",
      state: "recording",
      not_recording_reason: null,
      enable_setting: null,
      measured_tokens: 1000,
      turns: 30,
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
          input_tokens: 600,
          uncached_input_tokens: 120,
          output_tokens: 0,
          models: [
            {
              model: "haiku",
              turns: 20,
              input_tokens: 600,
              uncached_input_tokens: 120,
              output_tokens: 0,
              token_share_pct: 60,
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
    },
  };
}

function notRecordingState(): MissionControlState {
  const base = recordingState();
  return {
    ...base,
    kpi: {
      state: "not_recording",
      not_recording_reason: "disabled",
      enable_setting: "kpi.enabled",
      spend: null,
    },
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
    spend: {
      scope: "machine",
      state: "not_recording",
      not_recording_reason: "disabled",
      enable_setting: "kpi.enabled",
      measured_tokens: 0,
      turns: 0,
      providers: [],
    },
  };
}

const TASK_COUNTS: PlanTaskCounts = {
  pending: 1,
  in_progress: 0,
  in_review: 2,
  blocked: 1,
  paused: 1,
  done: 10,
  cancelled: 1,
  total: 16,
};

const NO_TASKS: PlanTaskCounts = {
  pending: 0,
  in_progress: 0,
  in_review: 0,
  blocked: 0,
  paused: 0,
  done: 0,
  cancelled: 0,
  total: 0,
};

function boardState(mission: MissionControlState, counts: PlanTaskCounts = TASK_COUNTS): PlanBoardState {
  return {
    plan_id: FIXTURE_PLAN.id,
    task_counts: counts,
    mission_control: mission,
  };
}

const PLAN: Plan = { ...FIXTURE_PLAN, milestone_kinds: [{ key: "skill", label: "Skill pinned", color: null }] };

function board(mission: MissionControlState, counts?: PlanTaskCounts) {
  return <MissionControlBoard plan={PLAN} state={boardState(mission, counts)} now={NOW} />;
}

beforeEach(() => {
  observers.length = 0;
});

describe("mission control board panels", () => {
  it("mission_control_board_paints_theme_backed_opaque_surface", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const root = host.querySelector('[data-testid="mission-control-board"]');
    expect(root?.classList.contains("mc-board")).toBe(true);

    const boardCss = readFileSync("src/components/Canvas/missionControl/missionControlBoard.css", "utf8");
    const boardRule = boardCss.match(/\.mc-board\s*\{[\s\S]*?\n\}/)?.[0] ?? "";
    expect(boardRule).toContain("background: var(--ct-bg);");
    expect(boardRule).not.toMatch(/background(?:-color)?\s*:\s*transparent/);
    expect(cssRule(boardCss, ".mc-board .mc-board-panel")).toContain("background: var(--ct-surface);");

    const appSource = readFileSync("src/App.tsx", "utf8");
    expect(appSource).toContain(
      'const isVisible = mainView === "workspace" && !pluginViewId && isActiveTab && !workspaceCanvasVisible;',
    );
    const terminalTabSource = readFileSync("src/components/Terminal/TerminalTab.tsx", "utf8");
    expect(terminalTabSource).toContain(
      'visibility: isVisible && terminalVisible && !historyReplaying ? "visible" : "hidden",',
    );
    await unmount();
  });

  it("opens straight onto the Needs you panel with no band above it", async () => {
    const { host, unmount } = await render(board(recordingState()));

    expect(host.querySelector('[data-testid="plan-board-status"]')).toBeNull();
    expect(host.querySelector('[data-testid="mission-control-board-actions-row"]')).toBeNull();
    expect(host.querySelector('[data-testid="mission-control-board-actions"]')).toBeNull();
    const root = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(Array.from(root.children).map((child) => child.getAttribute("data-testid"))).toEqual([
      "mc-board-needs-you",
      "mc-board-row",
      "mc-board-event-log",
    ]);
    await unmount();
  });

  it("asks for every open gate and held review in one Needs you panel", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const items = Array.from(host.querySelectorAll('[data-testid="mc-board-needs-you-item"]')) as HTMLElement[];
    expect(
      items.map((item) => [item.getAttribute("data-kind"), item.querySelector(".mc-board-needs-title")?.textContent]),
    ).toEqual([
      ["gate", "Ship the migration now?"],
      ["review", "Land the wave"],
      ["review", "Triage backlog"],
    ]);
    const facts = (item: HTMLElement) =>
      Array.from(item.querySelectorAll(".mc-board-fact")).map((fact) => fact.textContent);
    expect(facts(items[0]!)).toEqual(["decision gate", "waiting 2 h", "blocks 2 tasks"]);
    expect(facts(items[1]!)).toEqual(["review held", "waiting 1 h 07 m", "5 verdicts"]);
    expect(text(host, "mc-board-needs-you-count")).toBe("3");
    expect(text(host, "mc-board-needs-you")).not.toContain("·");
    await unmount();
  });

  it("leaves Needs you out entirely when nothing waits", async () => {
    const state = recordingState();
    state.held = { held_reviews: [], open_gates: [], oldest_hold_age_ms: null, verdicts_on_held: 0 };
    const { host, unmount } = await render(board(state));

    expect(host.querySelector('[data-testid="mc-board-needs-you"]')).toBeNull();
    const root = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(root.firstElementChild?.getAttribute("data-testid")).toBe("mc-board-row");
    await unmount();
  });

  it("reads this plan's new input, turns and quality without output, total or byte headlines", async () => {
    const { host, unmount } = await render(board(recordingState()));

    expect(text(host, "mc-board-plan-new-input")).toBe("1.5M");
    expect(text(host, "mc-board-plan-turns")).toBe("4,023");
    expect(text(host, "mc-board-bounces-amount")).toBe("1");
    expect(host.querySelector('[data-testid="mc-board-bounces-amount"]')?.classList.contains("mc-board-warn")).toBe(true);
    expect(text(host, "mc-board-done-first-try-amount")).toBe("10");
    expect(text(host, "mc-board-quality")).toBe("Qualitybounced1first try10 of 12");
    const progress = text(host, "mc-board-progress");
    for (const banned of ["5.3M", "1.4B", "65M", "output", "total", "bytes", "·"]) {
      expect(progress).not.toContain(banned);
    }
    expect(host.textContent?.toLowerCase()).not.toContain("provider allowance snapshots");
    await unmount();
  });

  it("shows only the statuses holding tasks and says nothing about the empty ones", async () => {
    const { host, unmount } = await render(board(recordingState()));

    expect(statusItems(host)).toEqual(["1pending", "2in review", "1blocked", "1paused", "10done", "1cancelled"]);
    expect(text(host, "mc-board-task-statuses")).not.toContain("nothing");
    expect(text(host, "mc-board-progress-meta")).toBe("16 tasks");
    await unmount();

    const paused = await render(board(recordingState(), { ...NO_TASKS, paused: 4, total: 4 }));
    expect(statusItems(paused.host)).toEqual(["4paused"]);
    expect(text(paused.host, "mc-board-task-statuses")).toBe("4paused");
    expect(
      paused.host.querySelector('[data-testid="mc-board-task-status-paused-count"]')?.classList.contains("mc-board-warn"),
    ).toBe(true);
    await paused.unmount();

    const empty = await render(board(recordingState(), NO_TASKS));
    expect(statusItems(empty.host)).toEqual([]);
    expect(text(empty.host, "mc-board-task-statuses")).toBe("No tasks yet.");
    await empty.unmount();
  });

  it("mission_control_spend_reads_each_complete_provider_allowance_window_weekly_first", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const gauges = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')) as HTMLElement[];
    expect(gauges.map((gauge) => gauge.getAttribute("data-window-key"))).toEqual([
      "claude-weekly",
      "claude-session",
      "codex-weekly",
    ]);
    expect(gauges.map((gauge) => gauge.textContent)).toEqual(["week 34%", "session 58%", "week 81%"]);
    const pct = (gauge: HTMLElement) => gauge.querySelector('[data-testid="mc-board-spend-window-pct"]') as HTMLElement;
    expect(pct(gauges[0]!).className).toBe("");
    expect(pct(gauges[2]!).classList.contains("mc-board-warn")).toBe(true);
    await unmount();
  });

  it("mission_control_spend_leaves_out_an_incomplete_allowance_window", async () => {
    const state = recordingState();
    state.spend.providers = [
      {
        ...state.spend.providers[0]!,
        allowance: { ...state.spend.providers[0]!.allowance!, session_resets_at: null },
      },
    ];
    const { host, unmount } = await render(board(state));

    const gauges = Array.from(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]'));
    expect(gauges.map((gauge) => gauge.getAttribute("data-window-key"))).toEqual(["claude-weekly"]);
    expect(text(host, "mc-board-spend")).not.toContain("—");
    await unmount();
  });

  it("groups by the declared area facet and names the untagged remainder", async () => {
    const { host, unmount } = await render(board(recordingState()));

    expect(areaRows(host)).toEqual([]);
    const toggle = await openAreas(host);
    expect(toggle.textContent).toBe("By area");
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(areaRows(host)).toEqual([
      { label: "Design", count: "6" },
      { label: "Backend", count: "2" },
      { label: "untagged", count: "2" },
    ]);
    const list = host.querySelector('[data-testid="mc-board-areas"] dl') as HTMLElement;
    expect(list.getAttribute("aria-label")).toBe("Area");
    expect(toggle.getAttribute("aria-controls")).toBe(list.id);
    expect(text(host, "mc-board-areas")).not.toContain("%");
    await act(async () => toggle.click());
    expect(areaRows(host)).toEqual([]);
    await unmount();
  });

  it("caps Needs you at three items behind Show all", async () => {
    const state = recordingState();
    state.held.held_reviews = [];
    state.held.open_gates = Array.from({ length: 5 }, (_, index) => ({
      id: `g${index}`,
      question: `Question ${index}?`,
      status: "open" as const,
      asked_at: NOW - H,
      age_ms: H,
      blocking_task_count: 0,
    }));
    const { host, unmount } = await render(board(state));

    const items = () => host.querySelectorAll('[data-testid="mc-board-needs-you-item"]');
    expect(items()).toHaveLength(3);
    expect(text(host, "mc-board-needs-you-count")).toBe("5");
    const more = host.querySelector('[data-testid="mc-board-needs-you-more"]') as HTMLButtonElement;
    expect(more.textContent).toBe("Show all 5");
    expect(more.getAttribute("aria-expanded")).toBe("false");
    await act(async () => more.click());
    expect(items()).toHaveLength(5);
    expect(more.textContent).toBe("Show fewer");
    await unmount();
  });

  it("shows the latest eight events and reveals the rest on request", async () => {
    const state = recordingState();
    state.event_log.entries = Array.from({ length: 12 }, (_, index) =>
      logEvent({ ts: DAY + H + index * 60_000, kind: "note", text: `note ${index}` }),
    );
    state.event_log.recorded_events = 12;
    const { host, unmount } = await render(board(state));

    const bodies = () => eventRows(host).map((row) => part(row, "mc-board-event-body")?.textContent);
    expect(bodies()).toEqual(Array.from({ length: 8 }, (_, index) => `note ${index + 4}`));
    expect(host.querySelector('[data-testid="mc-board-event-hour"]')).not.toBeNull();
    const more = host.querySelector('[data-testid="mc-board-event-log-more"]') as HTMLButtonElement;
    expect(more.textContent).toBe("Show all 12");
    await act(async () => more.click());
    expect(bodies()).toHaveLength(12);
    expect(text(host, "mc-board-event-log-meta")).toBe("12 events");
    await unmount();
  });
});

describe("mission control board fallback states", () => {
  it("states the ledger is not recording instead of showing a figure", async () => {
    const { host, unmount } = await render(board(notRecordingState()));

    expect(host.querySelector('[data-testid="mc-board-plan-new-input"]')).toBeNull();
    expect(host.querySelector('[data-testid="mc-board-plan-turns"]')).toBeNull();
    expect(text(host, "mc-board-plan-not-recording")).toContain("recording off");
    await unmount();
  });

  it("keeps the disabled KPI state concise and names its setting", async () => {
    const setting = "kpi.enabled";
    const state = notRecordingState();
    state.kpi = {
      state: "not_recording",
      not_recording_reason: "disabled",
      enable_setting: setting,
      spend: null,
    };
    const { host, unmount } = await render(board(state));

    expect(text(host, "mc-board-plan-not-recording")).toBe(`recording off, enable ${setting}`);
    expect(text(host, "mc-board-plan-spend")).not.toContain("—");
    await unmount();
  });

  it("never substitutes a zero for a missing figure in any not-recording reason", async () => {
    const reasons: KpiNotRecordingReason[] = [
      "disabled",
      "no_rows_for_plan",
      "unreadable",
      "not_read",
    ];
    const emptyCopy: Record<KpiNotRecordingReason, string> = {
      disabled: "recording off, enable kpi.enabled",
      no_rows_for_plan: "no plan ledger row",
      unreadable: "ledger unavailable",
      not_read: "ledger not read",
    };
    for (const reason of reasons) {
      const state = notRecordingState();
      state.kpi = {
        state: "not_recording",
        not_recording_reason: reason,
        enable_setting: reason === "disabled" ? "kpi.enabled" : null,
        spend: null,
      };
      const { host, unmount } = await render(board(state));

      expect(text(host, "mc-board-plan-not-recording")).toBe(emptyCopy[reason]);
      expect(text(host, "mc-board-plan-spend")).not.toMatch(/\b0\b/);
      expect(host.querySelector('[data-testid="mc-board-plan-turns"]')).toBeNull();
      await unmount();
    }
  });

  it("reads real ledger figures once recording is active", async () => {
    const state = recordingState();
    state.kpi = {
      state: "recording",
      not_recording_reason: null,
      enable_setting: null,
      spend: {
        events: 7,
        bytes: 4096,
        turns: 3,
        tokens: { input: 514_028, uncached_input: 25_600, output: 1800, total: 515_828 },
      },
    };
    const { host, unmount } = await render(board(state));

    expect(text(host, "mc-board-plan-new-input")).toBe("25.6K");
    expect(text(host, "mc-board-plan-turns")).toBe("3");
    expect(host.querySelector('[data-testid="mc-board-plan-not-recording"]')).toBeNull();
    await unmount();
  });

  it("falls back to stored executor roles under a Roles label", async () => {
    const { host, unmount } = await render(board(notRecordingState()));

    expect((await openAreas(host)).textContent).toBe("By role");
    expect(areaRows(host)).toEqual([
      { label: "frontend-dev", count: "2" },
      { label: "No role recorded", count: "1" },
    ]);
    expect(host.querySelector('[data-testid="mc-board-areas"] dl')?.getAttribute("aria-label")).toBe("Roles");
    expect(host.querySelectorAll('[data-testid="mc-board-areas"] p')).toHaveLength(0);
    expect(text(host, "mc-board-areas")).not.toContain("%");
    await unmount();
  });

  it("mission_control_spend_says_not_recording_instead_of_drawing_empty_gauges", async () => {
    const { host, unmount } = await render(board(notRecordingState()));

    expect(host.querySelectorAll('[data-testid="mc-board-spend-gauge"]')).toHaveLength(0);
    expect(host.querySelectorAll('[data-testid="mc-board-spend-model"]')).toHaveLength(0);
    expect(text(host, "mc-board-spend-empty")).toBe("not recording, enable kpi.enabled");
    expect(text(host, "mc-board-spend")).not.toContain("—");
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

const DECISION_TASK = "Deterministic Snake simulation";
const DECISION_REASON =
  "paused: Paused because deterministic simulation task_ulu1wz is blocked by repeated startup/dispatch coordination incidents";
const LONG_TASK = "CodeTerm route integration and final verification sweep";
const LONG_QUESTION = [
  "Should the orchestrator hold the release while the deterministic simulation lane is blocked",
  "on repeated startup and dispatch coordination incidents, or cut that lane out of the milestone",
  "and ship the remaining seven cards with a recorded follow-up task that names the missing",
  "verification, the exact focused gate command, and the two widths the board was observed at,",
  "so the next attempt starts from evidence instead of another discovery pass?",
].join(" ");

function logEvent(over: Partial<PlanEvent> & Pick<PlanEvent, "ts" | "kind" | "text">): PlanEvent {
  return {
    last_ts: over.ts,
    count: 1,
    author: "Pasteur",
    author_tab_id: "tab-planner",
    task_id: "t1",
    task_name: null,
    ...over,
  };
}

/** The three shapes the live board fused or ran on: decision, clipped name, long gate. */
function readableLogState(): MissionControlState {
  const state = recordingState();
  state.event_log.entries = [
    logEvent({
      ts: DAY + H,
      kind: "decision",
      task_id: "task_ulu1wz",
      task_name: DECISION_TASK,
      text: DECISION_REASON,
    }),
    logEvent({
      ts: DAY + H + 60_000,
      kind: "decision",
      task_id: "task_route",
      task_name: LONG_TASK,
      text: DECISION_REASON,
    }),
    logEvent({
      ts: DAY + H + 120_000,
      kind: "gate",
      author: "Planck",
      task_id: "task_gate",
      task_name: DECISION_TASK,
      text: LONG_QUESTION,
    }),
    logEvent({
      ts: DAY + H + 180_000,
      kind: "task_created",
      author: "Planner",
      task_id: "verify-clean-first-run",
      task_name: "Verify clean first run",
      text: "Verify clean first run",
    }),
  ];
  return state;
}

function eventRows(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll('[data-testid="mc-board-event-row"]'));
}

function part(row: HTMLElement, testId: string): HTMLElement | null {
  return row.querySelector(`[data-testid="${testId}"]`);
}

function revealByLabel(host: HTMLElement, label: string): HTMLButtonElement {
  const button = Array.from(host.querySelectorAll("button")).find(
    (candidate) => candidate.getAttribute("aria-label") === label,
  );
  if (!button) throw new Error(`no control labelled ${label}`);
  return button as HTMLButtonElement;
}

/** Reads one rule body out of the stylesheet without depending on its position. */
function cssRule(css: string, selector: string): string {
  const at = css.indexOf(`\n${selector} {`);
  return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
}

describe("mission control board event log readability", () => {
  it("prints a decision task and its reason as separate parts, never one fused string", async () => {
    const { host, unmount } = await render(board(readableLogState()));
    const row = eventRows(host)[0] as HTMLElement;

    const task = part(row, "mc-board-event-task") as HTMLElement;
    const body = part(row, "mc-board-event-body") as HTMLElement;
    expect(task.textContent).toBe(DECISION_TASK);
    expect(body.textContent?.startsWith("paused:")).toBe(true);
    expect(task.nextElementSibling?.textContent).toBe(EVENT_PART_SEPARATOR);
    expect(EVENT_PART_SEPARATOR).toMatch(/^\s.*\s$/);

    const rowText = row.textContent ?? "";
    expect(rowText).not.toContain(`${DECISION_TASK}paused:`);
    expect(rowText).toContain(`${DECISION_TASK}${EVENT_PART_SEPARATOR}paused:`);
    await unmount();
  });

  it("keeps the boundary visible when the task name is already ellipsised", async () => {
    const { host, unmount } = await render(board(readableLogState()));
    const row = eventRows(host)[1] as HTMLElement;

    expect(LONG_TASK.length).toBeGreaterThan(EVENT_TASK_NAME_MAX);
    const task = part(row, "mc-board-event-task") as HTMLElement;
    expect(task.textContent).toBe(`${LONG_TASK.slice(0, EVENT_TASK_NAME_MAX)}…`);

    const rowText = row.textContent ?? "";
    expect(rowText).not.toContain("…paused:");
    expect(rowText).toContain(`…${EVENT_PART_SEPARATOR}paused:`);
    await unmount();
  });

  it("bounds a long question to a preview and reveals the exact body on request", async () => {
    expect(LONG_QUESTION.length).toBeGreaterThan(400);
    const { host, unmount } = await render(board(readableLogState()));

    const row = eventRows(host)[2] as HTMLElement;
    const body = part(row, "mc-board-event-body") as HTMLElement;
    const preview = body.textContent ?? "";
    expect(preview.length).toBeLessThanOrEqual(EVENT_PREVIEW_MAX + 1);
    expect(preview.endsWith("…")).toBe(true);
    expect(LONG_QUESTION.startsWith(preview.slice(0, -1))).toBe(true);
    expect(row.textContent).not.toContain(LONG_QUESTION);

    const reveal = revealByLabel(row, EVENT_REVEAL_SHOW_LABEL);
    expect(reveal.tagName).toBe("BUTTON");
    expect(reveal.tabIndex).toBe(0);
    expect(reveal.getAttribute("aria-expanded")).toBe("false");
    expect(reveal.getAttribute("aria-controls")).toBe(body.id);

    await act(async () => reveal.click());

    const opened = eventRows(host)[2] as HTMLElement;
    const openedBody = part(opened, "mc-board-event-body") as HTMLElement;
    expect(openedBody.textContent).toBe(LONG_QUESTION);
    expect(openedBody.getAttribute("data-expanded")).toBe("true");
    expect(revealByLabel(opened, EVENT_REVEAL_HIDE_LABEL).getAttribute("aria-expanded")).toBe("true");

    expect(opened.querySelector(".mc-board-log-time")?.textContent).toBe(
      formatClock(DAY + H + 120_000),
    );
    expect(opened.querySelector(".mc-board-log-kind")?.textContent).toBe("Question");
    expect(part(opened, "mc-board-event-author")?.textContent).toBe("Planck");
    expect(part(opened, "mc-board-event-task")?.textContent).toBe(DECISION_TASK);
    await unmount();
  });

  it("keeps one visible title when the task name is the whole note", async () => {
    const { host, unmount } = await render(board(readableLogState()));
    const row = eventRows(host)[3] as HTMLElement;
    const title = "Verify clean first run";

    expect((row.textContent ?? "").split(title).length - 1).toBe(1);
    expect(part(row, "mc-board-event-task")).toBeNull();
    expect(part(row, "mc-board-event-body")?.textContent).toBe(title);
    expect(part(row, "mc-board-event-reveal")).toBeNull();
    await unmount();
  });

  it("keeps the preview bounded and the full body reachable at 390px", async () => {
    const { host, setWidth, unmount } = await render(board(readableLogState()));
    await setWidth(390);

    expect(
      host.querySelector('[data-testid="mission-control-board"]')?.getAttribute("data-board-layout"),
    ).toBe("narrow");

    const row = eventRows(host)[2] as HTMLElement;
    const body = part(row, "mc-board-event-body") as HTMLElement;
    expect((body.textContent ?? "").length).toBeLessThanOrEqual(EVENT_PREVIEW_MAX + 1);

    const reveal = revealByLabel(row, EVENT_REVEAL_SHOW_LABEL);
    await act(async () => reveal.click());

    const opened = eventRows(host)[2] as HTMLElement;
    expect(part(opened, "mc-board-event-body")?.textContent).toBe(LONG_QUESTION);
    expect(part(opened, "mc-board-event-task")?.textContent).toBe(DECISION_TASK);
    expect(opened.querySelector(".mc-board-log-time")?.textContent).toBe(
      formatClock(DAY + H + 120_000),
    );
    await unmount();
  });

  it("keeps every log row inside the card at both widths", () => {
    const css = readFileSync("src/components/Canvas/missionControl/missionControlBoard.css", "utf8");

    expect(cssRule(css, ".mc-board")).toContain("overflow-x: hidden;");
    expect(cssRule(css, ".mc-board-log-body")).toContain("text-overflow: ellipsis;");
    expect(cssRule(css, ".mc-board-log-more")).toContain("flex: 0 0 auto;");
    const openBody = cssRule(css, ".mc-board .mc-board-log-message-open .mc-board-log-body");
    expect(openBody).toContain("overflow-wrap: anywhere;");
    expect(openBody).toContain("white-space: pre-wrap;");
    expect(cssRule(css, ".mc-board-log-message-open")).toContain("flex-wrap: wrap;");
    expect(cssRule(css, '.mc-board[data-board-layout="narrow"] .mc-board-log-body')).toContain(
      "overflow-wrap: anywhere;",
    );
    expect(cssRule(css, '.mc-board[data-board-layout="narrow"] .mc-board-log-message')).toContain(
      "flex-wrap: wrap;",
    );
  });
});

describe("mission control board layout", () => {
  it("resolves a 390px viewport to the narrow, zero-minimum layout", () => {
    expect(boardLayout(390)).toBe("narrow");
    expect(boardLayout(BOARD_NARROW_MAX_PX)).toBe("narrow");
    expect(boardLayout(BOARD_NARROW_MAX_PX + 1)).toBe("wide");
    expect(BOARD_ROW_COLUMNS.narrow).toBe("minmax(0, 1fr)");
  });

  it("binds every grid track to a zero minimum at 390px so nothing can push the board wider", async () => {
    const { host, setWidth, unmount } = await render(board(recordingState()));
    await setWidth(390);

    const root = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(root.getAttribute("data-board-layout")).toBe("narrow");

    const row = host.querySelector('[data-testid="mc-board-row"]') as HTMLElement;
    const tracks = gridTemplate(row);

    expect(tracks).toContain("minmax(0, 1fr)");
    expect(tracks).not.toMatch(/minmax\(\s*(?!0)/);
    expect(tracks).not.toMatch(/\d+px/);
    await unmount();
  });

  it("returns to Progress beside Spend above the narrow bound", async () => {
    const { host, setWidth, unmount } = await render(board(recordingState()));
    await setWidth(390);
    await setWidth(1200);

    const root = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(root.getAttribute("data-board-layout")).toBe("wide");
    const row = host.querySelector('[data-testid="mc-board-row"]') as HTMLElement;
    expect(gridTemplate(row)).toContain("2fr");
    await unmount();
  });

  it("offers no drag affordances or form inputs on the board", async () => {
    const { host, unmount } = await render(board(recordingState()));

    const boardRoot = host.querySelector('[data-testid="mission-control-board"]') as HTMLElement;
    expect(boardRoot.querySelector("[draggable]")).toBeNull();
    expect(boardRoot.querySelector("input, select, textarea")).toBeNull();
    await unmount();
  });
});
