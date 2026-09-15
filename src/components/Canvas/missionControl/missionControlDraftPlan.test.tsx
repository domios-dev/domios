import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { __setListenForTest } from "../../../lib/bridge";
import type { ApiConfig } from "../../../lib/types";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { filterPlansForCanvas, isTeamActivePlan } from "../planTabMatch";
import { planDraftState } from "../planView/boardModel";
import type { Plan } from "../types";
import type { MissionControlState, PlanBoardState, TeamMissionControlState } from "./missionControlState";
import { MissionControlPlanSurface } from "./MissionControlPlanSurface";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const API: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "test-token", cors: false };
const TEAM_ID = "team-draft";
const PLAN_ID = "pl_draft";

const DRAFT: Plan = {
  ...FIXTURE_PLAN,
  id: PLAN_ID,
  title: "Fresh team plan",
  team_id: TEAM_ID,
  status: "active",
  mode: "planning",
  planning_state: "drafting",
  tasks: [],
  updated_at: 10,
};
const APPROVED: Plan = { ...DRAFT, mode: "approved", planning_state: "approved", updated_at: 20 };
const ARCHIVED: Plan = { ...DRAFT, id: "pl_old", title: "Old plan", status: "archived", updated_at: 5 };

type ListenHandler = (event: { payload: unknown }) => void;
const listeners = new Map<string, Set<ListenHandler>>();

function mission(recordedEvents: number): MissionControlState {
  return {
    held: { held_reviews: [], open_gates: [], oldest_hold_age_ms: null, verdicts_on_held: 0 },
    bounces: { total_attempts: 0, tasks_on_second_attempt: 0, first_try_done: 0, labelled_bounced: 0 },
    event_log: { recorded_events: recordedEvents, truncated: false, entries: [] },
    areas: { state: "no_facet", facet_key: null, facet_label: null, groups: [], untagged_task_count: 0, role_fallback: [] },
    kpi: { state: "not_recording", not_recording_reason: "disabled", enable_setting: "kpi.enabled", spend: null },
    spend: {
      scope: "machine",
      state: "not_recording",
      not_recording_reason: "no_rows_for_plan",
      enable_setting: null,
      measured_tokens: 0,
      turns: 0,
      providers: [],
    },
  } as MissionControlState;
}

function boardState(recordedEvents: number): PlanBoardState {
  return {
    plan_id: PLAN_ID,
    task_counts: { pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0, total: 0 },
    mission_control: mission(recordedEvents),
  };
}

function teamState(activePlan: Plan | null, archived: Plan[] = []): TeamMissionControlState {
  return {
    team_id: TEAM_ID,
    team: { id: TEAM_ID, name: "Draft team", lifecycle: "active", created_at: 1, workspace_id: "ws-test" },
    active_plan: activePlan,
    archived_plans: archived,
    events: [],
    next_cursor: null,
  } as TeamMissionControlState;
}

function ok(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value, text: async () => "" } as Response;
}

function missing(): Response {
  return { ok: false, status: 404, json: async () => ({}), text: async () => "not routed" } as Response;
}

interface Daemon {
  team: TeamMissionControlState;
  board: PlanBoardState;
}

function installDaemon(daemon: Daemon) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.includes(`/api/teams/${TEAM_ID}/mission-control/state`)) return ok(daemon.team);
    if (url.includes(`/api/plans/${PLAN_ID}/mode`) && init?.method === "POST") return ok({ ok: true });
    if (url.includes(`/api/plans/${PLAN_ID}/state`)) return ok(daemon.board);
    return missing();
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

async function flush() {
  for (let i = 0; i < 30; i++) await Promise.resolve();
}

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(ui);
    await flush();
  });
  return {
    host,
    root,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function byTestId(testId: string) {
  return document.body.querySelector<HTMLElement>(`[data-testid="${testId}"]`);
}

function surface(plans: Plan[]) {
  return (
    <MissionControlPlanSurface mode="workspace" teamId={TEAM_ID} workspaceId="ws-test" plans={plans} tabs={{}} api={API} />
  );
}

beforeEach(() => {
  listeners.clear();
  window.sessionStorage.clear();
  __setListenForTest(async (event, handler) => {
    const set = listeners.get(event) ?? new Set<ListenHandler>();
    listeners.set(event, set);
    set.add(handler as ListenHandler);
    return () => set.delete(handler as ListenHandler);
  });
});

afterEach(() => {
  __setListenForTest(null);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

describe("drafting active plan predicate", () => {
  it("keeps an Active draft in the team's active slot and leaves Archived plans out", () => {
    expect(isTeamActivePlan(DRAFT, TEAM_ID)).toBe(true);
    expect(isTeamActivePlan({ ...DRAFT, planning_state: "structure_ready" }, TEAM_ID)).toBe(true);
    expect(isTeamActivePlan(ARCHIVED, TEAM_ID)).toBe(false);
    expect(isTeamActivePlan(DRAFT, "team-other")).toBe(false);
    const kept = filterPlansForCanvas([ARCHIVED, DRAFT], "workspace", { teamId: TEAM_ID, workspaceId: "ws-test" });
    expect(kept.map((plan) => plan.id)).toEqual([PLAN_ID]);
  });

  it("marks only unapproved planning states as a draft", () => {
    expect(planDraftState(DRAFT)).toBe("drafting");
    expect(planDraftState({ ...DRAFT, planning_state: "structure_ready" })).toBe("structure_ready");
    expect(planDraftState({ ...DRAFT, planning_state: undefined })).toBe("drafting");
    expect(planDraftState(APPROVED)).toBeNull();
    expect(planDraftState({ ...DRAFT, mode: "execution" })).toBeNull();
    expect(planDraftState({ ...DRAFT, mode: "paused", planning_state: "structure_ready" })).toBe("structure_ready");
    expect(planDraftState({ ...DRAFT, mode: "paused", planning_state: "approved" })).toBeNull();
  });
});

describe("drafting active plan on the team Board", () => {
  it("shows the team's draft in the Plan box with a draft marker when the plan feed carries it", async () => {
    installDaemon({ team: teamState(DRAFT), board: boardState(1) });
    const { host, unmount } = await render(surface([DRAFT]));

    expect(host.querySelector('[data-testid="mc-board-plan-empty"]')).toBeNull();
    expect(byTestId("mc-board-plan-heading")?.textContent).toBe("Fresh team plan");
    const marker = byTestId("mc-board-plan-draft");
    expect(marker?.getAttribute("data-planning-state")).toBe("drafting");
    expect(marker?.textContent).toBe("Draft · Drafting");
    await unmount();
  });

  it("fills the Plan box from the team projection when the plan feed lacks the draft", async () => {
    installDaemon({ team: teamState(DRAFT), board: boardState(1) });
    const { host, unmount } = await render(surface([]));

    expect(host.querySelector('[data-testid="mc-board-plan-empty"]')).toBeNull();
    expect(byTestId("mc-board-plan-heading")?.textContent).toBe("Fresh team plan");
    expect(byTestId("mc-board-plan-draft")?.getAttribute("data-planning-state")).toBe("drafting");
    expect(byTestId("mission-control-board-actions")?.getAttribute("data-plan-id")).toBe(PLAN_ID);
    await unmount();
  });

  it("keeps an archived plan out of the active slot", async () => {
    installDaemon({ team: teamState(null, [ARCHIVED]), board: boardState(1) });
    const { unmount } = await render(surface([ARCHIVED]));

    expect(byTestId("mc-board-plan-empty")?.textContent).toBe("No active plan");
    expect(byTestId("mc-board-plan-heading")).toBeNull();
    expect(byTestId("mc-board-plan-draft")).toBeNull();
    expect(byTestId("mission-control-board-actions")).toBeNull();
    await unmount();
  });

  it("approves the draft through setPlanMode and keeps the same plan active without the marker", async () => {
    const daemon: Daemon = { team: teamState(DRAFT), board: boardState(1) };
    const fetchMock = installDaemon(daemon);
    const { unmount } = await render(surface([]));
    expect(byTestId("mc-board-plan-draft")).not.toBeNull();

    const trigger = byTestId("mission-control-board-menu-trigger") as HTMLButtonElement;
    trigger.focus();
    await act(async () => {
      trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    });
    const approve = Array.from(document.body.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find(
      (item) => item.textContent === "Approve plan",
    );
    expect(approve).toBeDefined();
    await act(async () => approve!.click());
    await act(async () => {
      byTestId("mission-control-approve-plan-dialog-confirm")?.click();
      await flush();
    });

    const modeCalls = fetchMock.mock.calls.filter(([url]) => String(url).endsWith(`/api/plans/${PLAN_ID}/mode`));
    expect(modeCalls).toHaveLength(1);
    expect(modeCalls[0]![1]?.method).toBe("POST");
    expect(JSON.parse(String(modeCalls[0]![1]?.body))).toEqual({ mode: "approved" });

    daemon.team = teamState(APPROVED);
    daemon.board = boardState(2);
    await act(async () => {
      for (const handler of listeners.get("plan-changed") ?? []) handler({ payload: { plan_id: PLAN_ID } });
      await flush();
    });

    expect(byTestId("mc-board-plan-draft")).toBeNull();
    expect(byTestId("mc-board-plan-heading")?.textContent).toBe("Fresh team plan");
    expect(byTestId("mission-control-board-actions")?.getAttribute("data-plan-id")).toBe(PLAN_ID);
    await unmount();
  });
});
