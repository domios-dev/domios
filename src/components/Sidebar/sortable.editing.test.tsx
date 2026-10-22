import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { SortableRow } from "./sortable";

const listeners = vi.hoisted(() => ({ onMouseDown: vi.fn(), onTouchStart: vi.fn(), onKeyDown: vi.fn() }));

vi.mock("@dnd-kit/sortable", async (original) => ({
  ...await original<typeof import("@dnd-kit/sortable")>(),
  useSortable: ({ disabled }: { disabled?: boolean }) => ({ setNodeRef: vi.fn(), attributes: { "aria-disabled": disabled }, listeners, transform: null, isDragging: false }),
}));
vi.mock("@dnd-kit/core", async (original) => ({
  ...await original<typeof import("@dnd-kit/core")>(),
  useDndContext: () => ({ activatorEvent: null }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("keeps mouse, touch and keyboard activation out of editors while ordinary row handles still drag", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  vi.clearAllMocks();
  try {
    await act(async () => root.render(<SortableRow id="fixture" data={{ group: "team", groupKey: "fixture" }}>
      <input aria-label="Team name" />
      <textarea aria-label="Description" />
      <select aria-label="Choice"><option>One</option></select>
      <div contentEditable suppressContentEditableWarning><span data-editor="nested">Editable</span></div>
      <div role="textbox"><span data-editor="custom">Custom editor</span></div>
      <span data-testid="handle">Drag row</span>
    </SortableRow>));
    const dispatch = async (target: Element) => {
      const mouse = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
      const touch = new Event("touchstart", { bubbles: true, cancelable: true });
      await act(async () => {
        target.dispatchEvent(mouse);
        target.dispatchEvent(touch);
        target.dispatchEvent(new KeyboardEvent("keydown", { key: " ", code: "Space", bubbles: true }));
      });
      expect(mouse.defaultPrevented).toBe(false);
      expect(touch.defaultPrevented).toBe(false);
    };
    for (const editor of host.querySelectorAll("input, textarea, select, [data-editor]")) await dispatch(editor);
    for (const listener of Object.values(listeners)) expect(listener).not.toHaveBeenCalled();
    await dispatch(host.querySelector('[data-dnd-group]')!);
    for (const listener of Object.values(listeners)) expect(listener).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("disabling protected-tab drag keeps nested tab actions enabled", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const clicked = vi.fn();
  try {
    await act(async () => root.render(<SortableRow id="main" disabled data={{ group: "tab", groupKey: "general" }}><button onClick={clicked}>Tab actions</button></SortableRow>));
    const action = host.querySelector("button")!;
    expect(action.closest('[aria-disabled="true"]')).toBeNull();
    await act(async () => action.click());
    expect(clicked).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
