import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MissionControlControls } from "./MissionControlControls";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Scope = "board" | "task";

const browserConfirm = vi.fn(() => true);
let restoreBrowserDialogs: () => void = () => undefined;

beforeEach(() => {
  const previousConfirm = window.confirm;
  window.confirm = browserConfirm;
  restoreBrowserDialogs = () => {
    window.confirm = previousConfirm;
  };
});

afterEach(() => {
  expect(browserConfirm).not.toHaveBeenCalled();
  restoreBrowserDialogs();
  vi.clearAllMocks();
});

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return {
    host,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function keydown(target: Element | null, key: string) {
  target?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
}

function byTestId<T extends HTMLElement = HTMLElement>(testId: string) {
  return document.body.querySelector<T>(`[data-testid="${testId}"]`);
}

function trigger(scope: Scope) {
  return byTestId<HTMLButtonElement>(`mission-control-${scope}-menu-trigger`)!;
}

async function openMenu(scope: Scope) {
  const button = trigger(scope);
  button.focus();
  await act(async () => keydown(button, "Enter"));
  const menu = byTestId(`mission-control-${scope}-menu`);
  expect(menu).not.toBeNull();
  return menu!;
}

function menuItems(menu: HTMLElement) {
  return Array.from(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
}

async function choose(scope: Scope, label: string) {
  const item = menuItems(await openMenu(scope)).find((candidate) => candidate.textContent === label);
  expect(item).toBeDefined();
  await act(async () => item!.click());
}

async function press(testId: string) {
  await act(async () => {
    byTestId<HTMLButtonElement>(testId)?.click();
    await Promise.resolve();
  });
}

function taskControls(actions: Partial<Parameters<typeof MissionControlControls>[0]> = {}) {
  return (
    <MissionControlControls
      scope="task"
      planId="plan-visible"
      taskId="task-1"
      taskName="Ship kebab"
      onDeleteTask={vi.fn()}
      {...actions}
    />
  );
}

describe("Mission Control controls", () => {
  it("opens board actions in a portal and confirms approve and clear in app", async () => {
    const approve = vi.fn();
    const clear = vi.fn();
    const { host, unmount } = await render(
      <MissionControlControls scope="board" planId="plan-visible" onApprovePlan={approve} onClearEverything={clear} />,
    );

    try {
      expect(trigger("board").tabIndex).toBe(0);
      expect(trigger("board").getAttribute("aria-label")).toBe("Board actions");
      const menu = await openMenu("board");
      expect(host.contains(menu)).toBe(false);
      expect(menu.getAttribute("aria-label")).toBe("Board actions");
      const items = menuItems(menu);
      expect(items.map((item) => item.textContent)).toEqual(["Approve plan", "Pause plan", "Clear everything"]);
      expect(items.map((item) => item.getAttribute("data-danger"))).toEqual([null, null, "true"]);
      expect(document.activeElement).toBe(items[0]);
      await act(async () => keydown(document.activeElement, "ArrowDown"));
      expect(document.activeElement).toBe(items[1]);
      await act(async () => keydown(document.activeElement, "ArrowUp"));
      await act(async () => keydown(document.activeElement, "ArrowUp"));
      expect(document.activeElement).toBe(items[2]);
      await act(async () => keydown(document.activeElement, "Escape"));
      expect(byTestId("mission-control-board-menu")).toBeNull();
      expect(document.activeElement).toBe(trigger("board"));

      await choose("board", "Approve plan");
      expect(byTestId("mission-control-approve-plan-dialog")?.getAttribute("role")).toBe("dialog");
      await press("mission-control-approve-plan-dialog-cancel");
      expect(byTestId("mission-control-approve-plan-dialog")).toBeNull();
      expect(approve).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(trigger("board"));

      await choose("board", "Approve plan");
      await press("mission-control-approve-plan-dialog-confirm");
      expect(approve).toHaveBeenCalledOnce();
      expect(approve).toHaveBeenCalledWith("plan-visible");

      await choose("board", "Clear everything");
      expect(byTestId("mission-control-clear-everything-dialog-confirm")?.className).toContain(
        "mc-lifecycle-button-danger",
      );
      await press("mission-control-clear-everything-dialog-confirm");
      expect(clear).toHaveBeenCalledOnce();
      expect(clear).toHaveBeenCalledWith("plan-visible");
    } finally {
      await unmount();
    }
  });

  it("task_pause_surface_removed", async () => {
    const { host, unmount } = await render(taskControls());

    try {
      expect(trigger("task").getAttribute("aria-label")).toBe("Actions for task Ship kebab");
      expect(trigger("task").getAttribute("aria-haspopup")).toBe("menu");
      const menu = await openMenu("task");
      expect(host.contains(menu)).toBe(false);
      expect(menu.getAttribute("aria-label")).toBe("Task actions for Ship kebab");
      const items = menuItems(menu);
      expect(items.map((item) => item.textContent)).toEqual(["Delete"]);
      expect(items[items.length - 1]?.getAttribute("data-danger")).toBe("true");
      expect(items.slice(0, -1).every((item) => item.getAttribute("data-danger") === null)).toBe(true);
    } finally {
      await unmount();
    }
  });

  it("renders no trigger when the task has no available action", async () => {
    const { host, unmount } = await render(
      <MissionControlControls scope="task" planId="plan-visible" taskId="task-1" />,
    );
    try {
      expect(host.querySelector('[data-testid="mission-control-task-menu-trigger"]')).toBeNull();
    } finally {
      await unmount();
    }
  });

  it("returns focus to the trigger when Escape closes the menu or a dialog", async () => {
    const remove = vi.fn();
    const { unmount } = await render(taskControls({ onDeleteTask: remove }));

    try {
      await openMenu("task");
      await act(async () => keydown(document.activeElement, "Escape"));
      expect(byTestId("mission-control-task-menu")).toBeNull();
      expect(document.activeElement).toBe(trigger("task"));

      await choose("task", "Delete");
      expect(document.activeElement).toBe(byTestId("mission-control-delete-task-dialog-cancel"));
      await act(async () => keydown(document.activeElement, "Escape"));
      expect(byTestId("mission-control-delete-task-dialog")).toBeNull();
      expect(remove).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(trigger("task"));

      await choose("task", "Delete");
      await press("mission-control-delete-task-dialog-confirm");
      expect(remove).toHaveBeenCalledOnce();
      expect(remove).toHaveBeenCalledWith("plan-visible", "task-1");
    } finally {
      await unmount();
    }
  });

  it("keeps the dialog open with the daemon error when an action fails", async () => {
    const remove = vi.fn(async () => {
      throw new Error("task delete failed: 409 caller rebound");
    });
    const { unmount } = await render(taskControls({ onDeleteTask: remove }));

    try {
      await choose("task", "Delete");
      await press("mission-control-delete-task-dialog-confirm");
      expect(remove).toHaveBeenCalledOnce();
      expect(byTestId("mission-control-delete-task-dialog-error")?.textContent).toBe(
        "task delete failed: 409 caller rebound",
      );
      expect(byTestId("mission-control-delete-task-dialog")).not.toBeNull();
      await press("mission-control-delete-task-dialog-cancel");
    } finally {
      await unmount();
    }
  });
});
