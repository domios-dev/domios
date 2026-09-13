import { act, useCallback, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LinkedText } from "../../../packages/chat-ui/src/codeterm/LinkedText";
import type { ApiConfig, Tab, Team, Workspace } from "../../lib/types";
import { NeedsYouNotificationRow } from "../Notifications/NeedsYouNotificationRow";
import { chatPlanRefView } from "../Terminal/chatPlanRef";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { canvasFocusStore, useCanvasWorkspaceTarget } from "./canvasFocusStore";
import { boardRefs } from "./missionControl/BoardRefText";
import { MissionControlPlanSurface } from "./missionControl/MissionControlPlanSurface";
import type { MissionControlState, PlanBoardState } from "./missionControl/missionControlState";
import { PlanViewShell } from "./PlanViewShell";
import type { PlanIndexEntry, ResolvedPlanRef } from "./plansApi";
import {
  findTaskByReference,
  taskDisplayKey,
  taskReferenceLabel,
  taskSegmentLabel,
} from "./taskDisplayKey";
import { openPlanTarget, type PlanTarget, type PlanTargetContext } from "./teamViewNavigation";
import { TaskStatus, type Plan, type Task } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TEAM_ID = "team-1";
const TEAM_WS = "ws-team";
const OTHER_WS = "ws-other";

const KEYED: Task = { ...FIXTURE_TASK, id: "task_seg1", key: "CT-7", name: "Render the segment", status: TaskStatus.Pending };
const LEGACY: Task = { ...FIXTURE_TASK, id: "task_legacy9", key: null, name: "Legacy task", status: TaskStatus.Pending };
const TEAM_PLAN: Plan = { ...FIXTURE_PLAN, id: "pl_team1", team_id: TEAM_ID, status: "active", tasks: [KEYED, LEGACY] };

const WORKSPACES: Workspace[] = [
  { id: OTHER_WS, name: "Other", tabs: [], active_tab_id: null, cwd: "/repo/other" },
  { id: TEAM_WS, name: "Team", tabs: ["orch-1"], active_tab_id: null, cwd: "/repo/team" },
];
const TEAMS = [{ id: TEAM_ID, name: "Team", lifecycle: "active", created_at: 0, workspace_id: TEAM_WS }] as unknown as Team[];
const INDEX = [
  { plan_id: TEAM_PLAN.id, team_id: TEAM_ID, canonical_project_path: "/repo/elsewhere", file_path: "", status: "active" },
] as unknown as PlanIndexEntry[];

function context(overrides: Partial<PlanTargetContext> = {}): PlanTargetContext {
  return {
    index: INDEX,
    workspaces: WORKSPACES,
    teams: TEAMS,
    activeWorkspaceId: TEAM_WS,
    setActiveWorkspace: vi.fn(),
    ...overrides,
  };
}

const mounted: Array<{ root: Root; host: HTMLElement }> = [];

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => root.render(ui));
  return host;
}

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  act(() => {
    canvasFocusStore.close();
    canvasFocusStore.closeWorkspace();
  });
  window.localStorage.clear();
  window.sessionStorage.clear();
  vi.unstubAllGlobals();
});

const SHELL_CTX = context();

function TeamView({ plans = [TEAM_PLAN] }: { plans?: Plan[] }) {
  const target = useCanvasWorkspaceTarget();
  const open = useCallback((next: PlanTarget) => {
    openPlanTarget(next, SHELL_CTX);
  }, []);
  return (
    <PlanViewShell
      mode="workspace"
      plans={plans}
      teamId={TEAM_ID}
      tabs={{}}
      workspaceId={TEAM_WS}
      workspaces={WORKSPACES}
      teams={TEAMS}
      workspaceTarget={target}
      onOpenPlanTarget={open}
    />
  );
}

function tabNames(host: HTMLElement): string[] {
  return Array.from(host.querySelectorAll('[data-testid="plan-view-mode-switch"] [role="tab"]')).map(
    (tab) => tab.textContent ?? "",
  );
}

function shellMode(host: HTMLElement): string | null {
  return host.querySelector('[data-testid="plan-view-shell"]')?.getAttribute("data-plan-view-mode") ?? null;
}

function modeTab(host: HTMLElement, value: string): HTMLButtonElement {
  const tab = host.querySelector<HTMLButtonElement>(`[role="tab"][data-value="${value}"]`);
  if (!tab) throw new Error(`no ${value} tab`);
  return tab;
}

async function openFromTasksBoard(host: HTMLElement, task: Task) {
  await act(async () => modeTab(host, "tasks").click());
  const card = host.querySelector<HTMLElement>(`[data-testid="plan-board-task-card"][data-task-id="${task.id}"]`);
  if (!card) throw new Error(`no card for ${task.id}`);
  await act(async () => card.click());
}

describe("task display key", () => {
  it("shows the stored key, or the uppercase id without task_ for a legacy task", () => {
    expect(taskDisplayKey(KEYED)).toBe("CT-7");
    expect(taskDisplayKey(LEGACY)).toBe("LEGACY9");
    expect(taskDisplayKey({ id: "t-bare" })).toBe("T-BARE");
    expect(taskReferenceLabel(KEYED)).toBe("CT-7 Render the segment");
    expect(taskSegmentLabel(LEGACY)).toBe("Task LEGACY9");
  });

  it("finds a task by exact id first and by case-folded key otherwise", () => {
    expect(findTaskByReference(TEAM_PLAN.tasks, "task_seg1")).toBe(KEYED);
    expect(findTaskByReference(TEAM_PLAN.tasks, "ct-7")).toBe(KEYED);
    expect(findTaskByReference(TEAM_PLAN.tasks, "legacy9")).toBe(LEGACY);
    expect(findTaskByReference(TEAM_PLAN.tasks, "CT-8")).toBeUndefined();
  });
});

describe("plan target navigation", () => {
  it("selects the owning team's workspace and lands on the exact task", () => {
    const setActiveWorkspace = vi.fn();
    canvasFocusStore.openFocus({ planId: TEAM_PLAN.id });
    const opened = openPlanTarget(
      { planId: TEAM_PLAN.id, taskId: KEYED.id },
      context({ activeWorkspaceId: OTHER_WS, setActiveWorkspace }),
    );
    expect(opened).toBe(true);
    expect(setActiveWorkspace).toHaveBeenCalledWith(TEAM_WS);
    expect(canvasFocusStore.isOpen()).toBe(false);
    expect(canvasFocusStore.isWorkspaceOpen()).toBe(true);
    expect(canvasFocusStore.getWorkspaceTarget()).toEqual({
      teamId: TEAM_ID,
      planId: TEAM_PLAN.id,
      jump: { view: "task", taskId: KEYED.id },
    });
  });

  it("opens a plan target on Plan and resolves a teamless plan by project ownership", () => {
    const setActiveWorkspace = vi.fn();
    const opened = openPlanTarget(
      { planId: "pl_solo", canonicalProjectPath: "/repo/other/pkg" },
      context({ index: [], setActiveWorkspace }),
    );
    expect(opened).toBe(true);
    expect(setActiveWorkspace).toHaveBeenCalledWith(OTHER_WS);
    expect(canvasFocusStore.getWorkspaceTarget()).toEqual({
      teamId: undefined,
      planId: "pl_solo",
      jump: { view: "plan" },
    });
  });

  it("leaves an unowned plan inert", () => {
    const setActiveWorkspace = vi.fn();
    expect(openPlanTarget({ planId: "pl_missing", taskId: "task_x" }, context({ index: [], setActiveWorkspace }))).toBe(false);
    expect(setActiveWorkspace).not.toHaveBeenCalled();
    expect(canvasFocusStore.getWorkspaceTarget()).toBeNull();
  });
});

describe("temporary task segment", () => {
  it("opens a Tasks card as one accessible Task <KEY> tab right after Tasks with the full detail", async () => {
    const host = await render(<TeamView />);
    await openFromTasksBoard(host, KEYED);

    expect(tabNames(host)).toEqual(["Board", "Spec", "Plan", "Tasks", "Task CT-7"]);
    const tab = modeTab(host, "task");
    expect(tab.getAttribute("aria-selected")).toBe("true");
    expect(tab.closest("[data-group]")?.previousElementSibling?.getAttribute("data-group")).toBe("plan");
    const close = host.querySelector('[data-testid="plan-view-temporary-tab-close"]') as HTMLButtonElement;
    expect(close.getAttribute("aria-label")).toBe("Close Task CT-7");

    const panel = host.querySelector('[role="tabpanel"]') as HTMLElement;
    expect(panel.getAttribute("aria-labelledby")).toBe(tab.id);
    expect(tab.getAttribute("aria-controls")).toBe(panel.id);
    expect(document.activeElement).toBe(panel);
    expect(panel.querySelector('[data-testid="plan-task-segment-key"]')?.textContent).toBe("CT-7");
    expect(panel.querySelector('[data-testid="plan-task-segment-title"]')?.textContent).toBe(KEYED.name);
    expect(panel.querySelector('[data-testid="canvas-detail-content"] [data-testid="status-chip"]')).not.toBeNull();
    expect(shellMode(host)).toBe("task");
    expect(host.querySelector('[data-testid="plan-board-view"]')?.parentElement?.hasAttribute("hidden")).toBe(true);
    expect(document.querySelector('[data-testid="plan-detail-overlay"]')).toBeNull();
  });

  it("closes with the close control and returns to Tasks with focus on the Tasks tab", async () => {
    const host = await render(<TeamView />);
    await openFromTasksBoard(host, KEYED);

    await act(async () => (host.querySelector('[data-testid="plan-view-temporary-tab-close"]') as HTMLButtonElement).click());
    expect(tabNames(host)).toEqual(["Board", "Spec", "Plan", "Tasks"]);
    expect(shellMode(host)).toBe("tasks");
    expect(document.activeElement).toBe(modeTab(host, "tasks"));
  });

  it("closes with Escape and reopens the same task on a later jump", async () => {
    const host = await render(<TeamView />);
    await openFromTasksBoard(host, KEYED);

    const panel = host.querySelector('[role="tabpanel"]') as HTMLElement;
    await act(async () => {
      panel.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(host.querySelector('[data-testid="plan-view-temporary-tab"]')).toBeNull();
    expect(shellMode(host)).toBe("tasks");

    await openFromTasksBoard(host, KEYED);
    expect(modeTab(host, "task").textContent).toBe("Task CT-7");
  });

  it("removes the temporary tab when another segment is chosen", async () => {
    const host = await render(<TeamView />);
    for (const mode of ["board", "spec", "plan"]) {
      await openFromTasksBoard(host, KEYED);
      await act(async () => modeTab(host, mode).click());
      expect(host.querySelector('[data-testid="plan-view-temporary-tab"]')).toBeNull();
      expect(shellMode(host)).toBe(mode);
    }
  });

  it("names a legacy task by its uppercase id fallback", async () => {
    const host = await render(<TeamView />);
    await openFromTasksBoard(host, LEGACY);
    expect(modeTab(host, "task").textContent).toBe("Task LEGACY9");
    expect(host.querySelector('[data-testid="plan-view-temporary-tab-close"]')?.getAttribute("aria-label")).toBe("Close Task LEGACY9");
  });

  it("a plan jump replaces the task segment with the Plan view", async () => {
    const host = await render(<TeamView />);
    await openFromTasksBoard(host, KEYED);
    await act(async () => {
      openPlanTarget({ planId: TEAM_PLAN.id }, SHELL_CTX);
    });
    expect(host.querySelector('[data-testid="plan-view-temporary-tab"]')).toBeNull();
    expect(shellMode(host)).toBe("plan");
  });

  it("a jump from outside the team view, such as a chat or Needs you link, opens the same segment", async () => {
    const host = await render(<TeamView />);
    await act(async () => {
      openPlanTarget({ planId: TEAM_PLAN.id, taskId: KEYED.id }, SHELL_CTX);
    });
    expect(tabNames(host)).toEqual(["Board", "Spec", "Plan", "Tasks", "Task CT-7"]);
    expect(host.querySelector('[data-testid="plan-task-segment"]')?.getAttribute("data-task-id")).toBe(KEYED.id);
  });
});

const API: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "test-token", cors: false };

function missionState(): MissionControlState {
  return {
    held: {
      held_reviews: [{ task_id: KEYED.id, name: KEYED.name, verdicts: 1, last_verdict_at: null, age_ms: 60_000 }],
      open_gates: [],
      oldest_hold_age_ms: 60_000,
      verdicts_on_held: 1,
    },
    bounces: { total_attempts: 0, tasks_on_second_attempt: 0, first_try_done: 0, labelled_bounced: 0 },
    event_log: {
      recorded_events: 1,
      truncated: false,
      entries: [{
        ts: 1_700_000_000_000,
        last_ts: 1_700_000_000_000,
        count: 1,
        kind: "decision",
        author: "Ada",
        author_tab_id: null,
        task_id: KEYED.id,
        task_name: KEYED.name,
        text: "paused: waiting on review",
      }],
    },
    areas: { state: "no_facet", facet_key: null, facet_label: null, groups: [], untagged_task_count: 0, role_fallback: [] },
    kpi: { state: "not_recording", not_recording_reason: "disabled", enable_setting: null, spend: null },
    spend: {
      scope: "machine",
      state: "not_recording",
      not_recording_reason: "disabled",
      enable_setting: null,
      measured_tokens: 0,
      turns: 0,
      providers: [],
    },
  } as MissionControlState;
}

const BOARD_STATE: PlanBoardState = {
  plan_id: TEAM_PLAN.id,
  task_counts: { pending: 2, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0, total: 2 },
  mission_control: missionState(),
};

async function settle() {
  for (let pass = 0; pass < 10; pass += 1) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

describe("Board jumps through the single navigation", () => {
  it("opens Needs you and What happened tasks by key through onOpenPlanTarget, never a side panel", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string) => (url.includes(`/api/plans/${TEAM_PLAN.id}/state`)
      ? { ok: true, status: 200, json: async () => BOARD_STATE, text: async () => "" }
      : { ok: false, status: 404, json: async () => ({}), text: async () => "not found" })));
    const onOpenPlanTarget = vi.fn();
    const plan = { ...TEAM_PLAN, owner: FIXTURE_PLAN.owner };
    const host = await render(
      <MissionControlPlanSurface
        mode="workspace"
        workspaceId="ws-test"
        workspaces={[{ id: "ws-test", name: "Test", tabs: ["orch-1"], active_tab_id: null, cwd: null }]}
        plans={[plan]}
        api={API}
        onOpenPlanTarget={onOpenPlanTarget}
      />,
    );
    await settle();

    const review = host.querySelector('[data-testid="mc-board-needs-you-item"][data-kind="review"] [data-testid="mc-board-ref"]') as HTMLButtonElement;
    expect(review.textContent).toBe("CT-7 Render the segment");
    await act(async () => review.click());

    const locator = host.querySelector('[data-testid="mc-board-event-task"] [data-testid="mc-board-ref"]') as HTMLButtonElement;
    expect(locator.textContent).toBe("CT-7 Render the segment");
    await act(async () => locator.click());

    expect(onOpenPlanTarget.mock.calls).toEqual([
      [{ planId: TEAM_PLAN.id, taskId: KEYED.id, teamId: TEAM_ID }],
      [{ planId: TEAM_PLAN.id, taskId: KEYED.id, teamId: TEAM_ID }],
    ]);
    expect(document.querySelector('[data-testid="plan-detail-overlay"]')).toBeNull();
  });

  it("names a task inside a Needs you notification by key and opens it without jumping to the chat", async () => {
    const onOpenTask = vi.fn();
    const onSelectTab = vi.fn();
    const refs = boardRefs([TEAM_PLAN], onOpenTask);
    const item = {
      key: "notification-n1",
      id: "n1",
      title: `Review ${KEYED.id} before merge`,
      state: { kind: "available" as const, tabId: "tab-a", workspaceId: TEAM_WS, messageId: "m-1" },
    };
    const tabs = { "tab-a": { id: "tab-a", workspace_id: TEAM_WS, agent_type: "claude" } as unknown as Tab };
    const host = await render(
      <ul>
        <NeedsYouNotificationRow item={item} refs={refs} tabs={tabs} onSelectTab={onSelectTab} />
      </ul>,
    );

    const link = host.querySelector('[data-testid="needs-you-notification-link"]') as HTMLElement;
    expect(link.getAttribute("aria-label")).toBe("Review CT-7 Render the segment before merge");
    const ref = link.querySelector('[data-testid="mc-board-ref"]') as HTMLButtonElement;
    expect(ref.textContent).toBe("CT-7 Render the segment");
    await act(async () => ref.click());
    await act(async () => {
      ref.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(onOpenTask).toHaveBeenCalledWith(KEYED.id);
    expect(onSelectTab).not.toHaveBeenCalled();
  });
});

describe("chat task links", () => {
  function resolvedTask(overrides: Partial<ResolvedPlanRef> = {}): ResolvedPlanRef {
    return {
      plan_id: TEAM_PLAN.id,
      target_id: KEYED.id,
      target_kind: "task",
      state: "resolved",
      title: KEYED.name,
      plan_title: TEAM_PLAN.title,
      task_title: KEYED.name,
      task_key: "CT-7",
      canonical_project_path: "/repo/team",
      ...overrides,
    };
  }

  it("labels a chat task link with the key before the title, falling back to the legacy id", async () => {
    expect(chatPlanRefView(resolvedTask(), [], WORKSPACES, false).title).toBe("CT-7 Render the segment");
    expect(chatPlanRefView(resolvedTask({ task_key: undefined }), [], WORKSPACES, false).title).toBe("SEG1 Render the segment");

    const opened: Array<[string, string | undefined, string | undefined]> = [];
    const view = chatPlanRefView(resolvedTask(), [], WORKSPACES, false);
    const host = await render(
      <LinkedText
        text={`Review ${KEYED.id} in ${TEAM_PLAN.id}`}
        resolvePlanRef={() => view}
        onOpenPlan={(planId, targetId, kind) => { opened.push([planId, targetId, kind]); }}
      />,
    );
    const link = host.querySelector('[data-testid="chat-task-link"]') as HTMLButtonElement;
    expect(link.textContent).toContain("CT-7 Render the segment");
    await act(async () => link.click());
    expect(opened).toEqual([[TEAM_PLAN.id, KEYED.id, "task"]]);
  });
});
