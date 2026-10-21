import { act } from "react";
import { createRoot } from "react-dom/client";
import { SortableContext } from "@dnd-kit/sortable";
import { afterEach, expect, it, vi } from "vitest";
import { keepPressFocusInPlace, SidebarDnd, SortableRow, type ReorderHandlers } from "./sortable";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const handlers: ReorderHandlers = { reorderWorkspaces: vi.fn(), reorderTabs: vi.fn(), reorderDisplay: vi.fn() };
const cleanups: Array<() => Promise<void>> = [];

afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.restoreAllMocks();
});

/** A team block holding a member row and a drag-disabled row, inside the sidebar list that owns the press guard. */
async function renderList() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  cleanups.push(async () => {
    await act(async () => root.unmount());
    host.remove();
  });
  await act(async () => root.render(
    <>
      <input aria-label="Composer" />
      <div role="list" aria-label="Sidebar list" onMouseDown={keepPressFocusInPlace}>
        <button type="button">Section toggle</button>
        <SidebarDnd handlers={handlers}>
          <SortableContext items={["team:alpha"]}>
            <SortableRow id="team:alpha" data={{ group: "tab", groupKey: "workspace" }}>
              <p>Workers</p>
              <SortableContext items={["tab:member", "tab:pinned"]}>
                <SortableRow id="tab:member" data={{ group: "team", groupKey: "alpha" }}>
                  <button type="button">Open member</button>
                  <input aria-label="Rename member" />
                </SortableRow>
                <SortableRow id="tab:pinned" disabled data={{ group: "team", groupKey: "alpha" }}>
                  <button type="button">Open pinned</button>
                </SortableRow>
              </SortableContext>
            </SortableRow>
          </SortableContext>
        </SidebarDnd>
      </div>
    </>,
  ));
  const byLabel = (selector: string) => host.querySelector<HTMLElement>(selector)!;
  return {
    host,
    composer: byLabel('input[aria-label="Composer"]'),
    rename: byLabel('input[aria-label="Rename member"]'),
    member: [...host.querySelectorAll("button")].find((button) => button.textContent === "Open member")!,
    pinned: [...host.querySelectorAll("button")].find((button) => button.textContent === "Open pinned")!,
    sectionToggle: [...host.querySelectorAll("button")].find((button) => button.textContent === "Section toggle")!,
    label: byLabel("p"),
  };
}

const pointer = (y: number, buttons = 1) => ({ bubbles: true, cancelable: true, button: 0, buttons, clientX: 20, clientY: y });

function press(target: Element) {
  const event = new MouseEvent("mousedown", pointer(100));
  act(() => { target.dispatchEvent(event); });
  return event;
}

it("a press anywhere in a sortable row withholds focus, so no wrapper is focused and scrolled into view", async () => {
  for (const pick of ["member", "pinned", "label"] as const) {
    const list = await renderList();
    list.composer.focus();
    expect(press(list[pick]).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(list.composer);
    await cleanups.pop()!();
  }
});

it("editable fields and controls outside sortable rows keep the browser's own press behaviour", async () => {
  const list = await renderList();
  expect(press(list.rename).defaultPrevented).toBe(false);
  expect(press(list.sectionToggle).defaultPrevented).toBe(false);
});

it("drags still start from a row and from the block around a drag-disabled row", async () => {
  for (const pick of ["member", "pinned"] as const) {
    const list = await renderList();
    act(() => {
      list[pick].dispatchEvent(new MouseEvent("mousedown", pointer(100)));
      document.dispatchEvent(new MouseEvent("mousemove", pointer(140)));
    });
    const dragging = list.host.querySelector('[data-dnd-dragging="true"]');
    act(() => { document.dispatchEvent(new MouseEvent("mouseup", pointer(140, 0))); });
    expect(dragging?.contains(list[pick])).toBe(true);
    expect(dragging?.querySelector("p") !== null).toBe(pick === "pinned");
    await cleanups.pop()!();
  }
});
