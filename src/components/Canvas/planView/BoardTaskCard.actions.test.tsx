import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan } from "../types";
import { buildBoardPlanModel } from "./boardModel";
import { BoardTaskCard } from "./BoardTaskCard";

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

function cardFor(status: TaskStatus) {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, id: "t-kebab", name: "Repair board layout", status }],
  };
  const card = buildBoardPlanModel(plan, {})
    .columns.flatMap((column) => column.cards)
    .find((entry) => entry.task.id === "t-kebab");
  if (!card) throw new Error("board card not found");
  return card;
}

async function openKebab(host: HTMLElement) {
  const card = host.querySelector<HTMLElement>('[data-testid="plan-board-task-card"]')!;
  const trigger = card.querySelector<HTMLButtonElement>('button[aria-label="Actions for task Repair board layout"]');
  expect(trigger).not.toBeNull();
  trigger!.focus();
  await act(async () => {
    trigger!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  });
  const menu = document.body.querySelector<HTMLElement>('[role="menu"][aria-label="Task actions for Repair board layout"]');
  expect(menu).not.toBeNull();
  return { card, trigger: trigger!, menu: menu! };
}

function labels(menu: HTMLElement) {
  return Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent);
}

describe("board task card kebab", () => {
  it("names its trigger and opens task actions in a portal outside the card", async () => {
    const onOpen = vi.fn();
    const { host, unmount } = await render(
      <BoardTaskCard
        card={cardFor(TaskStatus.Pending)}
        planId="plan-1"
        onOpen={onOpen}
        onPauseTask={vi.fn()}
        onResumeTask={vi.fn()}
        onDeleteTask={vi.fn()}
      />,
    );

    try {
      const { card, trigger, menu } = await openKebab(host);
      expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
      expect(trigger.closest(".plan-board-task-actions")).not.toBeNull();
      expect(card.contains(menu)).toBe(false);
      expect(labels(menu)).toEqual(["Pause", "Delete"]);
      expect(onOpen).not.toHaveBeenCalled();
    } finally {
      await unmount();
    }
  });

  it("offers Resume and Delete on a paused card", async () => {
    const { host, unmount } = await render(
      <BoardTaskCard
        card={cardFor(TaskStatus.Paused)}
        planId="plan-1"
        onPauseTask={vi.fn()}
        onResumeTask={vi.fn()}
        onDeleteTask={vi.fn()}
      />,
    );

    try {
      const { menu } = await openKebab(host);
      expect(labels(menu)).toEqual(["Resume", "Delete"]);
    } finally {
      await unmount();
    }
  });

  it("renders no kebab when no task action is wired", async () => {
    const { host, unmount } = await render(<BoardTaskCard card={cardFor(TaskStatus.Pending)} planId="plan-1" />);
    try {
      expect(host.querySelector('button[aria-haspopup="menu"]')).toBeNull();
    } finally {
      await unmount();
    }
  });
});
