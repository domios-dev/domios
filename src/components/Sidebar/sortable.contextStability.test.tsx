import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { SortableContext, useSortable } from "@dnd-kit/sortable";
import { SidebarDnd, type ReorderHandlers } from "./sortable";

const handlers: ReorderHandlers = { reorderWorkspaces: () => {}, reorderTabs: () => {}, reorderDisplay: () => {} };
let rowRenders = 0;

function Row() {
  rowRenders++;
  const { setNodeRef } = useSortable({ id: "tab:a" });
  return <div ref={setNodeRef} />;
}

const rows = <SortableContext items={["tab:a"]}><Row /></SortableContext>;

describe("sidebar drag context", () => {
  it("leaves sortable rows alone when the sidebar re-renders without a drag", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<SidebarDnd handlers={handlers}>{rows}</SidebarDnd>));
    const before = rowRenders;
    for (let render = 0; render < 3; render++) {
      await act(async () => root.render(<SidebarDnd handlers={handlers}>{rows}</SidebarDnd>));
    }
    expect(rowRenders).toBe(before);
    await act(async () => root.unmount());
  });
});
