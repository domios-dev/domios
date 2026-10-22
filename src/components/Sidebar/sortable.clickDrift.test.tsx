import { act } from "react";
import { createRoot } from "react-dom/client";
import { SortableContext } from "@dnd-kit/sortable";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SidebarDnd, SortableRow, type ReorderHandlers } from "./sortable";
import { TeamUnit } from "./TeamUnit";
import { FIXTURE_PLAN } from "../Canvas/cards/fixtures.testdata";
import { TabItem } from "./TabItem";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const handlers: ReorderHandlers = { reorderWorkspaces: vi.fn(), reorderTabs: vi.fn(), reorderDisplay: vi.fn() };

beforeEach(() => vi.useFakeTimers());
afterEach(async () => {
  await act(async () => { vi.runOnlyPendingTimers(); });
  vi.useRealTimers();
});

async function pressAndRelease(drift: number) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const opened = vi.fn();
  try {
    await act(async () => root.render(
      <SidebarDnd handlers={handlers}>
        <SortableContext items={["tab:first", "tab:second"]}>
          {["first", "second"].map((id) => (
            <SortableRow key={id} id={`tab:${id}`} data={{ group: "tab", groupKey: "workspace" }}>
              <button type="button" onClick={() => opened(id)}>Open {id}</button>
            </SortableRow>
          ))}
        </SortableContext>
      </SidebarDnd>,
    ));
    const button = host.querySelector("button")!;
    const at = (y: number) => ({ bubbles: true, cancelable: true, button: 0, buttons: 1, clientX: 20, clientY: y });
    await act(async () => {
      button.dispatchEvent(new MouseEvent("mousedown", at(100)));
      document.dispatchEvent(new MouseEvent("mousemove", at(100 + drift)));
    });
    const dragging = host.querySelector('[data-dnd-dragging="true"]') !== null;
    await act(async () => {
      document.dispatchEvent(new MouseEvent("mouseup", { ...at(100 + drift), buttons: 0 }));
      button.click();
    });
    return { dragging, opened: opened.mock.calls.map(([id]) => id) };
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
}

it("a click whose pointer drifts like a trackpad press still opens the pressed row", async () => {
  for (const drift of [0, 4, 6, 9]) {
    expect(await pressAndRelease(drift)).toEqual({ dragging: false, opened: ["first"] });
  }
});

it.each([["Enter", "Enter"], [" ", "Space"]])("keeps %s on nested tab activation while the row itself can keyboard drag", async (key, code) => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const opened = vi.fn();
  try {
    await act(async () => root.render(
      <SidebarDnd handlers={handlers}>
        <SortableContext items={["tab:fixture"]}>
          <SortableRow id="tab:fixture" data={{ group: "tab", groupKey: "workspace" }}>
            <TabItem tab={{ id: "fixture", title: "Shell", tab_type: "Terminal", pty_id: null, cwd: null }}
              index={0} isActive={false} onClick={opened} onClose={() => undefined} canClose workspaceId="workspace" />
          </SortableRow>
        </SortableContext>
      </SidebarDnd>,
    ));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label^="Open tab "]')!;
    button.focus();
    expect(document.activeElement).toBe(button);
    const down = new KeyboardEvent("keydown", { key, code, bubbles: true, cancelable: true });
    await act(async () => {
      button.dispatchEvent(down);
      button.dispatchEvent(new KeyboardEvent("keyup", { key, code, bubbles: true, cancelable: true }));
      if (!down.defaultPrevented) button.click();
    });
    expect(down.defaultPrevented).toBe(false);
    expect(opened).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[data-dnd-dragging="true"]')).toBeNull();
    const row = host.querySelector<HTMLElement>('[data-dnd-group]')!;
    row.focus();
    await act(async () => { row.dispatchEvent(new KeyboardEvent("keydown", { key, code, bubbles: true, cancelable: true })); });
    expect(host.querySelector('[data-dnd-dragging="true"]')).not.toBeNull();
    await act(async () => { vi.runOnlyPendingTimers(); });
    await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true })); });
    expect(host.querySelector('[data-dnd-dragging="true"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("a deliberate drag still starts dragging instead of opening the row", async () => {
  expect(await pressAndRelease(40)).toEqual({ dragging: true, opened: [] });
});

it.each([["Enter", "Enter"], [" ", "Space"]])("keeps %s activation on the Team card button", async (key, code) => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onOpenEffort = vi.fn();
  try {
    await act(async () => root.render(
      <SidebarDnd handlers={handlers}>
        <SortableContext items={["team:fixture"]}>
          <SortableRow id="team:fixture" data={{ group: "team", groupKey: "workspace" }}>
            <TeamUnit team={{ id: "fixture", name: "Release", lifecycle: "active", created_at: 0, workspace_id: "workspace" }}
              plan={{ ...FIXTURE_PLAN, title: "Release" }} tabs={{}} managers={null} managerCount={0}
              workers={null} workerCount={0} workingWorkerCount={0}
              providers={[]} onOpenEffort={onOpenEffort} />
          </SortableRow>
        </SortableContext>
      </SidebarDnd>,
    ));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="Open Release"]')!;
    button.focus();
    expect(document.activeElement).toBe(button);
    const down = new KeyboardEvent("keydown", { key, code, bubbles: true, cancelable: true });
    const up = new KeyboardEvent("keyup", { key, code, bubbles: true, cancelable: true });
    await act(async () => {
      button.dispatchEvent(down);
      button.dispatchEvent(up);
      // Happy DOM needs the browser's uncancelled native button default action supplied explicitly.
      if (!down.defaultPrevented && !up.defaultPrevented) button.click();
    });
    expect(down.defaultPrevented).toBe(false);
    expect(host.querySelector('[data-dnd-dragging="true"]')).toBeNull();
    expect(onOpenEffort.mock.calls).toEqual([[{ teamId: "fixture", managerTabId: null, planId: FIXTURE_PLAN.id }]]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
