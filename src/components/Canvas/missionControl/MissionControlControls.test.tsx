import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { MissionControlControls } from "./MissionControlControls";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

async function openMenu(host: HTMLElement, triggerId: string) {
  const trigger = host.querySelector(`[data-testid="${triggerId}"]`) as HTMLButtonElement;
  expect(trigger).not.toBeNull();
  expect(trigger.tabIndex).toBe(0);
  await act(async () => trigger.click());
  const menu = host.querySelector('[role="menu"]') as HTMLElement;
  expect(menu).not.toBeNull();
  expect(document.activeElement).toBe(menu.querySelector('[role="menuitem"]'));
  return menu;
}

describe("Mission Control confirmed controls", () => {
  it("exposes the exact board actions and keeps every cancelled action inert", async () => {
    const approve = vi.fn();
    const pause = vi.fn(async () => undefined);
    const preview = vi.fn(() => new Promise<never>(() => undefined));
    const clear = vi.fn();
    const confirm = vi.fn(() => false);
    const previousConfirm = window.confirm;
    window.confirm = confirm;
    const { host, unmount } = await render(
      <MissionControlControls
        scope="board"
        planId="plan-visible"
        onApprovePlan={approve}
        onPreviewPlanLifecycle={preview}
        onConfirmPlanLifecycle={pause}
        onClearEverything={clear}
      />,
    );

    try {
      const expected = ["Approve plan", "Pause plan", "Clear everything"];
      for (const label of expected) {
        const menu = await openMenu(host, "mission-control-board-menu-trigger");
        expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).toEqual(expected);
        expect(menu.querySelector('[role="menuitem"]:last-child')?.className).toContain(
          "mission-control-actions-menuitem-danger",
        );
        const item = Array.from(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find(
          (candidate) => candidate.textContent === label,
        );
        expect(item).not.toBeUndefined();
        await act(async () => item?.click());
        const cancel = document.body.querySelector<HTMLButtonElement>(
          '[data-testid="mission-control-plan-pause-cancel"]',
        );
        if (label === "Pause plan") {
          expect(cancel).not.toBeNull();
          await act(async () => cancel?.click());
        } else {
          expect(cancel).toBeNull();
        }
      }

      expect(confirm).toHaveBeenCalledTimes(expected.length - 1);
      expect(approve).not.toHaveBeenCalled();
      expect(pause).not.toHaveBeenCalled();
      expect(clear).not.toHaveBeenCalled();
    } finally {
      window.confirm = previousConfirm;
      await unmount();
    }
  });

  it("exposes confirmed Delete for a task and confirms one disposable target through the callback seam", async () => {
    const deleteTask = vi.fn();
    const confirm = vi.fn(() => false);
    const previousConfirm = window.confirm;
    window.confirm = confirm;
    const { host, unmount } = await render(
      <MissionControlControls
        scope="task"
        planId="plan-visible"
        taskId="task-disposable"
        onDeleteTask={deleteTask}
      />,
    );

    try {
      const menu = await openMenu(host, "mission-control-task-menu-trigger");
      expect(host.querySelector('[data-testid="mission-control-task-actions"]')?.getAttribute("data-plan-id")).toBe(
        "plan-visible",
      );
      expect(host.querySelector('[data-testid="mission-control-task-actions"]')?.getAttribute("data-task-id")).toBe(
        "task-disposable",
      );
      expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).toEqual(["Delete"]);
      expect(menu.querySelector('[role="menuitem"]')?.className).toContain(
        "mission-control-actions-menuitem-danger",
      );
      await act(async () => menu.querySelector<HTMLButtonElement>('[role="menuitem"]')?.click());
      expect(confirm).toHaveBeenCalledTimes(1);
      expect(deleteTask).not.toHaveBeenCalled();

      confirm.mockReturnValue(true);
      const confirmedMenu = await openMenu(host, "mission-control-task-menu-trigger");
      await act(async () => confirmedMenu.querySelector<HTMLButtonElement>('[role="menuitem"]')?.click());
      expect(deleteTask).toHaveBeenCalledOnce();
      expect(deleteTask).toHaveBeenCalledWith("plan-visible", "task-disposable");
    } finally {
      window.confirm = previousConfirm;
      await unmount();
    }
  });

  it("keeps the menu bounded for keyboard interaction at 390px", async () => {
    const { host, unmount } = await render(
      <div style={{ width: "390px", maxWidth: "390px", overflowX: "hidden" }}>
        <MissionControlControls scope="board" planId="plan-phone" />
      </div>,
    );
    try {
      const menu = await openMenu(host, "mission-control-board-menu-trigger");
      expect(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).toHaveLength(3);
      await act(async () => {
        menu.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
      });
      expect(document.activeElement?.textContent).toBe("Pause plan");
      const css = readFileSync("src/components/Canvas/missionControl/missionControlBoard.css", "utf8");
      expect(css).toContain("max-width: calc(100vw - 24px)");
      expect(css).toContain("min-width: min(12rem, calc(100vw - 24px))");
      expect(css).not.toContain("border-left");
    } finally {
      await unmount();
    }
  });
});
