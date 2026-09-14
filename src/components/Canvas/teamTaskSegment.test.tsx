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
import { PlanSidePanelLayer } from "./PlanSidePanelHost";
import { planSidePanelStore } from "./planSidePanelStore";
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
    planSidePanelStore.close();
    canvasFocusStore.close();
    canvasFocusStore.closeWorkspace();
  });
  window.localStorage.clear();
  window.sessionStorage.clear();
  vi.unstubAllGlobals();
});

const SHELL_CTX = context();

/** The team view with the app-level side panel layer above it, both driven by openPlanTarget. */
function TeamView({ plans = [TEAM_PLAN], ctx = SHELL_CTX, api }: { plans?: Plan[]; ctx?: PlanTargetContext; api?: ApiConfig }) {
  const target = useCanvasWorkspaceTarget();
  const open = useCallback((next: PlanTarget) => {
    openPlanTarget(next, ctx);
  }, [ctx]);
  return (
    <>
      <PlanViewShell
        mode="workspace"
        plans={plans}
        teamId={TEAM_ID}
        tabs={{}}
        workspaceId={TEAM_WS}
        workspaces={WORKSPACES}
        teams={TEAMS}
        workspaceTarget={target}
        api={api}
        onOpenPlanTarget={open}
      />
      <PlanSidePanelLayer plans={plans} tabs={{}} onOpenPlanTarget={open} />
    </>
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

function sidePanel(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[data-testid="plan-side-panel"]');
}

function sidePanelControl(name: "full" | "close"): HTMLButtonElement {
  const control = document.querySelector<HTMLButtonElement>(`[data-testid="plan-side-panel-${name}"]`);
  if (!control) throw new Error(`no side panel ${name} control`);
  return control;
}

function taskCard(host: HTMLElement, task: Task): HTMLElement {
  const card = host.querySelector<HTMLElement>(`[data-testid="plan-board-task-card"][data-task-id="${task.id}"]`);
  if (!card) throw new Error(`no card for ${task.id}`);
  return card;
}

async function pressEscape(target: Element) {
  await act(async () => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
}

async function openFromTasksBoard(host: HTMLElement, task: Task): Promise<HTMLElement> {
  if (shellMode(host) !== "tasks") await act(async () => modeTab(host, "tasks").click());
  const card = taskCard(host, task);
  card.focus();
  await act(async () => card.click());
  return card;
}

async function openFull(host: HTMLElement, task: Task) {
  await openFromTasksBoard(host, task);
  await act(async () => sidePanelControl("full").click());
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
  it("opens a task in the side panel by default without selecting a workspace or the team view", () => {
    const setActiveWorkspace = vi.fn();
    const opened = openPlanTarget(
      { planId: TEAM_PLAN.id, taskId: KEYED.id },
      context({ activeWorkspaceId: OTHER_WS, setActiveWorkspace }),
    );
    expect(opened).toBe(true);
    expect(setActiveWorkspace).not.toHaveBeenCalled();
    expect(canvasFocusStore.isWorkspaceOpen()).toBe(false);
    expect(canvasFocusStore.getWorkspaceTarget()).toBeNull();
    expect(planSidePanelStore.get()?.target).toEqual({ planId: TEAM_PLAN.id, taskId: KEYED.id, teamId: TEAM_ID });
  });

  it("full selects the owning team's workspace, closes the side panel and lands on the exact task", () => {
    const setActiveWorkspace = vi.fn();
    openPlanTarget({ planId: TEAM_PLAN.id, taskId: KEYED.id }, context());
    canvasFocusStore.openFocus({ planId: TEAM_PLAN.id });
    const opened = openPlanTarget(
      { planId: TEAM_PLAN.id, taskId: KEYED.id, presentation: "full" },
      context({ activeWorkspaceId: OTHER_WS, setActiveWorkspace }),
    );
    expect(opened).toBe(true);
    expect(planSidePanelStore.get()).toBeNull();
    expect(setActiveWorkspace).toHaveBeenCalledWith(TEAM_WS);
    expect(canvasFocusStore.isOpen()).toBe(false);
    expect(canvasFocusStore.isWorkspaceOpen()).toBe(true);
    expect(canvasFocusStore.getWorkspaceTarget()).toEqual({
      teamId: TEAM_ID,
      planId: TEAM_PLAN.id,
      jump: { view: "task", taskId: KEYED.id },
    });
  });

  it("full opens a plan target on Plan and resolves a teamless plan by project ownership", () => {
    const setActiveWorkspace = vi.fn();
    const opened = openPlanTarget(
      { planId: "pl_solo", canonicalProjectPath: "/repo/other/pkg", presentation: "full" },
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

  it("leaves an unowned plan inert in both presentations", () => {
    const setActiveWorkspace = vi.fn();
    const ctx = context({ index: [], setActiveWorkspace });
    expect(openPlanTarget({ planId: "pl_missing", taskId: "task_x" }, ctx)).toBe(false);
    expect(openPlanTarget({ planId: "pl_missing", presentation: "full" }, ctx)).toBe(false);
    expect(setActiveWorkspace).not.toHaveBeenCalled();
    expect(planSidePanelStore.get()).toBeNull();
    expect(canvasFocusStore.getWorkspaceTarget()).toBeNull();
  });
});

describe("shared task side panel", () => {
  it("opens a Tasks card in the side panel over Tasks with the key, title, detail and named controls", async () => {
    const host = await render(<TeamView />);
    await openFromTasksBoard(host, KEYED);

    expect(shellMode(host)).toBe("tasks");
    expect(tabNames(host)).toEqual(["Board", "Spec", "Plan", "Tasks"]);
    const panel = sidePanel() as HTMLElement;
    expect(panel.getAttribute("role")).toBe("dialog");
    expect(panel.getAttribute("aria-label")).toBe("Task CT-7 Render the segment");
    expect(document.activeElement).toBe(panel);
    expect(panel.querySelector('[data-testid="plan-side-panel-key"]')?.textContent).toBe("CT-7");
    expect(panel.querySelector('[data-testid="plan-side-panel-title"]')?.textContent).toBe(KEYED.name);
    const header = panel.querySelector("header") as HTMLElement;
    expect(header.firstElementChild?.getAttribute("data-testid")).toBe("plan-side-panel-key");
    expect(panel.querySelector('[data-testid="canvas-detail-content"] [data-testid="status-chip"]')).not.toBeNull();
    expect(sidePanelControl("full").getAttribute("aria-label")).toBe("Open Task CT-7 in full view");
    expect(sidePanelControl("close").getAttribute("aria-label")).toBe("Close Task CT-7");
    expect(host.querySelector('[data-testid="plan-task-segment"]')).toBeNull();
  });

  it("Escape closes the side panel and returns focus to the card that opened it", async () => {
    const host = await render(<TeamView />);
    const card = await openFromTasksBoard(host, KEYED);

    await pressEscape(sidePanel() as HTMLElement);
    expect(sidePanel()).toBeNull();
    expect(shellMode(host)).toBe("tasks");
    expect(document.activeElement).toBe(card);
  });

  it("the close control closes the side panel and returns focus to the opener", async () => {
    const host = await render(<TeamView />);
    const card = await openFromTasksBoard(host, KEYED);

    await act(async () => sidePanelControl("close").click());
    expect(sidePanel()).toBeNull();
    expect(document.activeElement).toBe(card);
  });

  it("opening a second task replaces the content and keeps the first opener for focus return", async () => {
    const host = await render(<TeamView />);
    const first = await openFromTasksBoard(host, KEYED);
    await act(async () => taskCard(host, LEGACY).click());

    expect(document.querySelectorAll('[data-testid="plan-side-panel"]').length).toBe(1);
    expect(sidePanel()?.getAttribute("data-task-id")).toBe(LEGACY.id);
    expect(document.querySelector('[data-testid="plan-side-panel-key"]')?.textContent).toBe("LEGACY9");
    expect(sidePanelControl("full").getAttribute("aria-label")).toBe("Open Task LEGACY9 in full view");

    await pressEscape(sidePanel() as HTMLElement);
    expect(document.activeElement).toBe(first);
  });

  it("opens over whichever team view mode is selected without changing it", async () => {
    const host = await render(<TeamView />);
    for (const mode of ["board", "spec", "plan", "tasks"]) {
      await act(async () => modeTab(host, mode).click());
      await act(async () => {
        openPlanTarget({ planId: TEAM_PLAN.id, taskId: KEYED.id }, SHELL_CTX);
      });
      expect(shellMode(host)).toBe(mode);
      expect(sidePanel()?.getAttribute("data-task-id")).toBe(KEYED.id);
      await pressEscape(sidePanel() as HTMLElement);
    }
  });

  it("opens a plan in the side panel with its title and a show full control that lands on Plan", async () => {
    const host = await render(<TeamView />);
    await act(async () => modeTab(host, "board").click());
    await act(async () => {
      openPlanTarget({ planId: TEAM_PLAN.id }, SHELL_CTX);
    });

    const panel = sidePanel() as HTMLElement;
    expect(panel.getAttribute("data-kind")).toBe("plan");
    expect(panel.querySelector('[data-testid="plan-side-panel-title"]')?.textContent).toBe(TEAM_PLAN.title);
    expect(panel.querySelector('[data-testid="canvas-detail-content"]')).not.toBeNull();
    expect(shellMode(host)).toBe("board");
    expect(sidePanelControl("full").getAttribute("aria-label")).toBe("Open plan in full view");
    expect(sidePanelControl("close").getAttribute("aria-label")).toBe("Close plan");

    await act(async () => sidePanelControl("full").click());
    expect(sidePanel()).toBeNull();
    expect(shellMode(host)).toBe("plan");
  });
});

describe("temporary task segment through show full", () => {
  it("show full replaces the side panel with one accessible Task <KEY> tab right after Tasks", async () => {
    const host = await render(<TeamView />);
    await openFull(host, KEYED);

    expect(sidePanel()).toBeNull();
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
    expect(shellMode(host)).toBe("task");
    expect(host.querySelector('[data-testid="plan-board-view"]')?.parentElement?.hasAttribute("hidden")).toBe(true);
  });

  it("closes with the close control and returns to Tasks with focus on the Tasks tab", async () => {
    const host = await render(<TeamView />);
    await openFull(host, KEYED);

    await act(async () => (host.querySelector('[data-testid="plan-view-temporary-tab-close"]') as HTMLButtonElement).click());
    expect(tabNames(host)).toEqual(["Board", "Spec", "Plan", "Tasks"]);
    expect(shellMode(host)).toBe("tasks");
    expect(document.activeElement).toBe(modeTab(host, "tasks"));
  });

  it("closes with Escape and reaches the same task again through the side panel", async () => {
    const host = await render(<TeamView />);
    await openFull(host, KEYED);

    await pressEscape(host.querySelector('[role="tabpanel"]') as HTMLElement);
    expect(host.querySelector('[data-testid="plan-view-temporary-tab"]')).toBeNull();
    expect(shellMode(host)).toBe("tasks");

    await openFull(host, KEYED);
    expect(modeTab(host, "task").textContent).toBe("Task CT-7");
  });

  it("removes the temporary tab when another segment is chosen", async () => {
    const host = await render(<TeamView />);
    for (const mode of ["board", "spec", "plan"]) {
      await openFull(host, KEYED);
      await act(async () => modeTab(host, mode).click());
      expect(host.querySelector('[data-testid="plan-view-temporary-tab"]')).toBeNull();
      expect(shellMode(host)).toBe(mode);
    }
  });

  it("names a legacy task by its uppercase id fallback", async () => {
    const host = await render(<TeamView />);
    await openFull(host, LEGACY);
    expect(modeTab(host, "task").textContent).toBe("Task LEGACY9");
    expect(host.querySelector('[data-testid="plan-view-temporary-tab-close"]')?.getAttribute("aria-label")).toBe("Close Task LEGACY9");
  });

  it("a full plan jump replaces the task segment with the Plan view", async () => {
    const host = await render(<TeamView />);
    await openFull(host, KEYED);
    await act(async () => {
      openPlanTarget({ planId: TEAM_PLAN.id, presentation: "full" }, SHELL_CTX);
    });
    expect(host.querySelector('[data-testid="plan-view-temporary-tab"]')).toBeNull();
    expect(shellMode(host)).toBe("plan");
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
  it("opens Needs you and What happened tasks by key through onOpenPlanTarget with no surface overlay", async () => {
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

  it("a Needs you task on the Board opens the shared side panel over Board", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string) => (url.includes(`/api/plans/${TEAM_PLAN.id}/state`)
      ? { ok: true, status: 200, json: async () => BOARD_STATE, text: async () => "" }
      : { ok: false, status: 404, json: async () => ({}), text: async () => "not found" })));
    const plan = { ...TEAM_PLAN, owner: FIXTURE_PLAN.owner };
    const host = await render(<TeamView plans={[plan]} api={API} />);
    await act(async () => modeTab(host, "board").click());
    await settle();

    const review = host.querySelector('[data-testid="mc-board-needs-you-item"][data-kind="review"] [data-testid="mc-board-ref"]') as HTMLButtonElement;
    review.focus();
    await act(async () => review.click());
    expect(shellMode(host)).toBe("board");
    expect(sidePanel()?.getAttribute("data-task-id")).toBe(KEYED.id);

    await pressEscape(sidePanel() as HTMLElement);
    expect(document.activeElement).toBe(review);
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

describe("chat task and plan links", () => {
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

  /** A chat surface outside the team view, wired the way TabSessionChat opens plan refs. */
  function ChatSurface({ text, ctx }: { text: string; ctx: PlanTargetContext }) {
    const open = useCallback((next: PlanTarget) => {
      openPlanTarget(next, ctx);
    }, [ctx]);
    const view = chatPlanRefView(resolvedTask(), [], WORKSPACES, false);
    return (
      <>
        <LinkedText
          text={text}
          resolvePlanRef={() => view}
          onOpenPlan={(planId, targetId, kind) => open({ planId, taskId: kind === "task" ? targetId : undefined })}
        />
        <PlanSidePanelLayer plans={[TEAM_PLAN]} tabs={{}} onOpenPlanTarget={open} />
      </>
    );
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

  it("a chat task link opens the side panel without navigating, Escape returns to the link, show full navigates", async () => {
    const setActiveWorkspace = vi.fn();
    const ctx = context({ activeWorkspaceId: OTHER_WS, setActiveWorkspace });
    const host = await render(<ChatSurface text={`Review ${KEYED.id} in ${TEAM_PLAN.id}`} ctx={ctx} />);

    const link = host.querySelector('[data-testid="chat-task-link"]') as HTMLButtonElement;
    link.focus();
    await act(async () => link.click());
    expect(sidePanel()?.getAttribute("data-task-id")).toBe(KEYED.id);
    expect(setActiveWorkspace).not.toHaveBeenCalled();
    expect(canvasFocusStore.isWorkspaceOpen()).toBe(false);
    expect(canvasFocusStore.getWorkspaceTarget()).toBeNull();

    await pressEscape(sidePanel() as HTMLElement);
    expect(sidePanel()).toBeNull();
    expect(document.activeElement).toBe(link);

    await act(async () => link.click());
    await act(async () => sidePanelControl("full").click());
    expect(sidePanel()).toBeNull();
    expect(setActiveWorkspace).toHaveBeenCalledWith(TEAM_WS);
    expect(canvasFocusStore.getWorkspaceTarget()).toEqual({
      teamId: TEAM_ID,
      planId: TEAM_PLAN.id,
      jump: { view: "task", taskId: KEYED.id },
    });
  });

  it("a chat plan link opens the plan side panel without navigating, and show full lands on Plan", async () => {
    const setActiveWorkspace = vi.fn();
    const ctx = context({ activeWorkspaceId: OTHER_WS, setActiveWorkspace });
    const host = await render(<ChatSurface text={`Plan ${TEAM_PLAN.id} is ready`} ctx={ctx} />);

    const link = host.querySelector('[data-testid="chat-plan-link"]') as HTMLButtonElement;
    link.focus();
    await act(async () => link.click());
    expect(sidePanel()?.getAttribute("data-kind")).toBe("plan");
    expect(sidePanelControl("full").getAttribute("aria-label")).toBe("Open plan in full view");
    expect(setActiveWorkspace).not.toHaveBeenCalled();
    expect(canvasFocusStore.getWorkspaceTarget()).toBeNull();

    await pressEscape(sidePanel() as HTMLElement);
    expect(document.activeElement).toBe(link);

    await act(async () => link.click());
    await act(async () => sidePanelControl("full").click());
    expect(setActiveWorkspace).toHaveBeenCalledWith(TEAM_WS);
    expect(canvasFocusStore.getWorkspaceTarget()).toEqual({
      teamId: TEAM_ID,
      planId: TEAM_PLAN.id,
      jump: { view: "plan" },
    });
  });
});
