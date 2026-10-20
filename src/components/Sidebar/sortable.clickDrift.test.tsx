import { act } from "react";
import { createRoot } from "react-dom/client";
import { SortableContext } from "@dnd-kit/sortable";
import { expect, it, vi } from "vitest";
import { SidebarDnd, SortableRow, type ReorderHandlers } from "./sortable";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const handlers: ReorderHandlers = { reorderWorkspaces: vi.fn(), reorderTabs: vi.fn(), reorderDisplay: vi.fn() };

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

it("a deliberate drag still starts dragging instead of opening the row", async () => {
  expect(await pressAndRelease(40)).toEqual({ dragging: true, opened: [] });
});
