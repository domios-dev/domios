import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PlanLifecycleKind, PlanLifecyclePreview } from "../planLifecycle";
import { MissionControlControls } from "./MissionControlControls";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const EFFECTS: PlanLifecyclePreview["effects"] = [
  "plan_reminders",
  "heartbeat_nudges",
  "system_notifications",
  "team_schedules",
];

const PAUSE_PREVIEW: PlanLifecyclePreview = {
  kind: "pause",
  plan_id: "plan-live",
  plan_title: "Ship pause and resume",
  mode: "execution",
  applicable: true,
  not_applicable_reason: null,
  operation_id: null,
  restore_mode: null,
  agents: [
    { tab_id: "tab-orch", name: "Mira", role: "orchestrator", available: true },
    { tab_id: "tab-plan", name: "Plan Keeper", role: "planner", available: true },
    { tab_id: "tab-work", name: "Lane Worker", role: "worker", available: true },
  ],
  schedules: [{ job_id: "job-digest", name: "Team digest", prior_enabled: true }],
  effects: EFFECTS,
};

const RESUME_PREVIEW: PlanLifecyclePreview = {
  ...PAUSE_PREVIEW,
  kind: "resume",
  mode: "paused",
  operation_id: "pz_record",
  restore_mode: "execution",
  agents: [
    { tab_id: "tab-orch", name: "Mira", role: "orchestrator", available: true },
    { tab_id: "tab-plan", name: "Plan Keeper", role: "planner", available: true },
    { tab_id: "tab-gone", name: "Gone Worker", role: "worker", available: false },
  ],
};

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return {
    host,
    rerender: async (next: ReactElement) => act(async () => root.render(next)),
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function trigger(host: HTMLElement) {
  return host.querySelector<HTMLButtonElement>('[data-testid="mission-control-board-menu-trigger"]')!;
}

async function openMenu(host: HTMLElement) {
  const menuTrigger = trigger(host);
  menuTrigger.focus();
  await act(async () => keydown(menuTrigger, "Enter"));
  return Array.from(
    document.body.querySelectorAll<HTMLButtonElement>('[data-testid="mission-control-board-menu"] [role="menuitem"]'),
  );
}

async function closeMenu() {
  await act(async () => keydown(document.activeElement, "Escape"));
}

async function choose(host: HTMLElement, label: string) {
  const item = (await openMenu(host)).find((candidate) => candidate.textContent === label);
  expect(item).not.toBeUndefined();
  await act(async () => item?.click());
  await act(async () => {
    await Promise.resolve();
  });
}

function dialog(kind: PlanLifecycleKind) {
  return document.body.querySelector<HTMLElement>(`[data-testid="mission-control-plan-${kind}-dialog"]`);
}

function button(testId: string) {
  return document.body.querySelector<HTMLButtonElement>(`[data-testid="${testId}"]`);
}

function agentRows(root: HTMLElement) {
  return Array.from(root.querySelectorAll<HTMLElement>('[data-testid="mission-control-plan-lifecycle-agent"]')).map(
    (row) => ({
      tabId: row.getAttribute("data-tab-id"),
      available: row.getAttribute("data-available"),
      text: row.textContent,
    }),
  );
}

function listAttr(root: HTMLElement, testId: string, attr: string) {
  return Array.from(root.querySelectorAll(`[data-testid="${testId}"]`)).map((row) => row.getAttribute(attr));
}

function keydown(target: Element | null, key: string, shiftKey = false) {
  target?.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey, bubbles: true }));
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Mission Control plan pause and resume dialogs", () => {
  it("renders the pause preview in a body portal and keeps cancel and Escape inert", async () => {
    const loadPreview = vi.fn(async () => PAUSE_PREVIEW);
    const confirmLifecycle = vi.fn(async () => undefined);
    const windowConfirm = vi.spyOn(window, "confirm");
    const { host, unmount } = await render(
      <MissionControlControls
        scope="board"
        planId="plan-live"
        onPreviewPlanLifecycle={loadPreview}
        onConfirmPlanLifecycle={confirmLifecycle}
      />,
    );

    try {
      await choose(host, "Pause plan");
      const pause = dialog("pause");
      expect(pause).not.toBeNull();
      expect(host.contains(pause)).toBe(false);
      expect(pause?.closest(".mc-lifecycle-backdrop")?.parentElement).toBe(document.body);
      expect(pause?.getAttribute("role")).toBe("dialog");
      expect(pause?.getAttribute("aria-modal")).toBe("true");
      expect(loadPreview).toHaveBeenCalledOnce();
      expect(loadPreview).toHaveBeenCalledWith("plan-live", "pause");

      expect(agentRows(pause!)).toEqual([
        { tabId: "tab-orch", available: "true", text: "MiraOrchestrator" },
        { tabId: "tab-plan", available: "true", text: "Plan KeeperPlanner" },
        { tabId: "tab-work", available: "true", text: "Lane WorkerWorker" },
      ]);
      expect(listAttr(pause!, "mission-control-plan-lifecycle-schedule", "data-job-id")).toEqual(["job-digest"]);
      expect(listAttr(pause!, "mission-control-plan-lifecycle-effect", "data-effect")).toEqual(EFFECTS);
      expect(document.activeElement).toBe(button("mission-control-plan-pause-cancel"));

      await act(async () => button("mission-control-plan-pause-cancel")?.click());
      expect(dialog("pause")).toBeNull();
      expect(document.activeElement).toBe(trigger(host));

      await choose(host, "Pause plan");
      expect(dialog("pause")).not.toBeNull();
      await act(async () => keydown(document.activeElement, "Escape"));
      expect(dialog("pause")).toBeNull();
      expect(document.activeElement).toBe(trigger(host));

      expect(confirmLifecycle).not.toHaveBeenCalled();
      expect(windowConfirm).not.toHaveBeenCalled();
    } finally {
      await unmount();
    }
  });

  it("confirms a pause through the single authoritative callback", async () => {
    const loadPreview = vi.fn(async () => PAUSE_PREVIEW);
    const confirmLifecycle = vi.fn(async () => undefined);
    const approve = vi.fn();
    const clear = vi.fn();
    const { host, unmount } = await render(
      <MissionControlControls
        scope="board"
        planId="plan-live"
        onApprovePlan={approve}
        onClearEverything={clear}
        onPreviewPlanLifecycle={loadPreview}
        onConfirmPlanLifecycle={confirmLifecycle}
      />,
    );

    try {
      await choose(host, "Pause plan");
      await act(async () => button("mission-control-plan-pause-confirm")?.click());

      expect(confirmLifecycle).toHaveBeenCalledOnce();
      expect(confirmLifecycle).toHaveBeenCalledWith("plan-live", "pause");
      expect(loadPreview).toHaveBeenCalledTimes(1);
      expect(dialog("pause")).toBeNull();
      expect(approve).not.toHaveBeenCalled();
      expect(clear).not.toHaveBeenCalled();
    } finally {
      await unmount();
    }
  });

  it("lists recorded agents on resume, marks a dead tab unavailable, and omits post-pause joiners", async () => {
    const loadPreview = vi.fn(async () => RESUME_PREVIEW);
    const confirmLifecycle = vi.fn(async () => undefined);
    const { host, unmount } = await render(
      <MissionControlControls
        scope="board"
        planId="plan-live"
        planPaused
        onPreviewPlanLifecycle={loadPreview}
        onConfirmPlanLifecycle={confirmLifecycle}
      />,
    );

    try {
      const labels = (await openMenu(host)).map((item) => item.textContent);
      expect(labels).toEqual(["Approve plan", "Resume plan", "Clear everything"]);
      await closeMenu();

      await choose(host, "Resume plan");
      const resume = dialog("resume");
      expect(resume).not.toBeNull();
      expect(loadPreview).toHaveBeenCalledOnce();
      expect(loadPreview).toHaveBeenCalledWith("plan-live", "resume");
      expect(agentRows(resume!)).toEqual([
        { tabId: "tab-orch", available: "true", text: "MiraOrchestrator" },
        { tabId: "tab-plan", available: "true", text: "Plan KeeperPlanner" },
        { tabId: "tab-gone", available: "false", text: "Gone WorkerWorkerUnavailable" },
      ]);
      expect(resume?.textContent).not.toContain("Lane Worker");
      expect(listAttr(resume!, "mission-control-plan-lifecycle-schedule", "data-job-id")).toEqual(["job-digest"]);
      expect(listAttr(resume!, "mission-control-plan-lifecycle-effect", "data-effect")).toEqual(EFFECTS);
      expect(resume?.querySelector('[data-testid="mission-control-plan-resume-dialog-restore-mode"]')).not.toBeNull();

      await act(async () => button("mission-control-plan-resume-cancel")?.click());
      expect(confirmLifecycle).not.toHaveBeenCalled();

      await choose(host, "Resume plan");
      await act(async () => button("mission-control-plan-resume-confirm")?.click());
      expect(confirmLifecycle).toHaveBeenCalledOnce();
      expect(confirmLifecycle).toHaveBeenCalledWith("plan-live", "resume");
      expect(dialog("resume")).toBeNull();
    } finally {
      await unmount();
    }
  });

  it("disables confirm for a not-applicable preview", async () => {
    const loadPreview = vi.fn(async () => ({
      ...RESUME_PREVIEW,
      mode: "execution" as const,
      applicable: false,
      not_applicable_reason: "Plan is not paused",
      agents: [],
      schedules: [],
      effects: [],
    }));
    const confirmLifecycle = vi.fn(async () => undefined);
    const { host, unmount } = await render(
      <MissionControlControls
        scope="board"
        planId="plan-live"
        planPaused
        onPreviewPlanLifecycle={loadPreview}
        onConfirmPlanLifecycle={confirmLifecycle}
      />,
    );

    try {
      await choose(host, "Resume plan");
      expect(button("mission-control-plan-resume-confirm")?.disabled).toBe(true);
      expect(
        document.body.querySelector('[data-testid="mission-control-plan-resume-dialog-not-applicable"]'),
      ).not.toBeNull();
      await act(async () => button("mission-control-plan-resume-confirm")?.click());
      expect(confirmLifecycle).not.toHaveBeenCalled();
      await act(async () => button("mission-control-plan-resume-cancel")?.click());
    } finally {
      await unmount();
    }
  });

  it("traps focus in the dialog and leaves approve and clear reachable afterwards", async () => {
    const approve = vi.fn();
    const clear = vi.fn();
    const confirmLifecycle = vi.fn(async () => undefined);
    const windowConfirm = vi.spyOn(window, "confirm");
    const { host, unmount } = await render(
      <MissionControlControls
        scope="board"
        planId="plan-live"
        onApprovePlan={approve}
        onClearEverything={clear}
        onPreviewPlanLifecycle={vi.fn(async () => PAUSE_PREVIEW)}
        onConfirmPlanLifecycle={confirmLifecycle}
      />,
    );

    try {
      await choose(host, "Pause plan");
      const cancel = button("mission-control-plan-pause-cancel")!;
      const confirm = button("mission-control-plan-pause-confirm")!;
      expect(document.activeElement).toBe(cancel);

      confirm.focus();
      await act(async () => keydown(confirm, "Tab"));
      expect(document.activeElement).toBe(cancel);
      await act(async () => keydown(cancel, "Tab", true));
      expect(document.activeElement).toBe(confirm);

      await act(async () => cancel.click());
      expect(confirmLifecycle).not.toHaveBeenCalled();

      const labels = (await openMenu(host)).map((item) => item.textContent);
      expect(labels).toEqual(["Approve plan", "Pause plan", "Clear everything"]);
      await closeMenu();
      await choose(host, "Approve plan");
      await act(async () => button("mission-control-approve-plan-dialog-confirm")?.click());
      await choose(host, "Clear everything");
      await act(async () => button("mission-control-clear-everything-dialog-confirm")?.click());
      expect(approve).toHaveBeenCalledOnce();
      expect(approve).toHaveBeenCalledWith("plan-live");
      expect(clear).toHaveBeenCalledOnce();
      expect(clear).toHaveBeenCalledWith("plan-live");
      expect(windowConfirm).not.toHaveBeenCalled();
    } finally {
      await unmount();
    }
  });
});
