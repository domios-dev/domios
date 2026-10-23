import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { commitSidebarDrag, sameGroupCollision, SortableRow } from "./sortable";
import { SidebarDragPreview } from "./SidebarDragPreview";

const state = vi.hoisted(() => ({ active: "tab:a", over: "tab:b" }));
vi.mock("@dnd-kit/core", async importOriginal => ({
  ...await importOriginal<typeof import("@dnd-kit/core")>(),
  useDndContext: () => ({ active: { id: state.active }, over: { id: state.over } }),
  DragOverlay: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("@dnd-kit/sortable", async importOriginal => ({
  ...await importOriginal<typeof import("@dnd-kit/sortable")>(),
  useSortable: ({ id }: { id: string }) => ({ setNodeRef: () => {}, attributes: {}, listeners: {},
    isDragging: id === state.active, items: ["tab:a", "tab:b"], transform: { x: 0, y: 70 }, transition: "transform 200ms" }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("draws a separate inert preview without duplicating live row identities", async () => {
  const source = document.createElement("div");
  source.setAttribute("data-dnd-id", "tab:a");
  source.innerHTML = '<button id="own-row">Owner</button><button data-dnd-group="child:a">Browser</button>';
  document.body.append(source);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<SidebarDragPreview />));
    const preview = host.querySelector('[aria-hidden="true"]');
    expect(preview?.textContent).toBe("OwnerBrowser");
    expect(preview?.hasAttribute("inert")).toBe(true);
    expect(preview?.querySelector("[id], [data-dnd-id], [data-dnd-group]")).toBeNull();
    expect(source.querySelector("#own-row")).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    source.remove();
  }
});

it("keeps the row and browser child in place while only the absolute marker changes", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const tree = () => <SortableRow id="tab:b" data={{ group: "tab", groupKey: "workspace" }}>
    <button>Owner</button><button>Owner's Browser</button>
  </SortableRow>;
  try {
    await act(async () => root.render(tree()));
    const row = host.firstElementChild as HTMLElement;
    const browser = host.querySelectorAll("button")[1];
    expect(row.style.transform).toBe("");
    expect(row.querySelector('[data-dnd-indicator="bottom"]')).not.toBeNull();
    state.over = "tab:a";
    await act(async () => root.render(tree()));
    expect(host.firstElementChild).toBe(row);
    expect(host.querySelectorAll("button")[1]).toBe(browser);
    expect(row.style.transform).toBe("");
    expect(row.querySelector("[data-dnd-indicator]")).toBeNull();
  } finally {
    state.over = "tab:b";
    await act(async () => root.unmount());
    host.remove();
  }
});

it("wires fixed geometry collision to the same committed order and ignores nested groups", () => {
  const data = { group: "tab", groupKey: "workspace", sortable: { items: ["tab:a", "tab:b", "tab:c"] } };
  const active = { id: "tab:c", data: { current: data } };
  const containers = ["tab:a", "tab:b", "tab:c"].map(id => ({ id, disabled: false, data: { current: data } }));
  containers.push({ id: "child", disabled: false, data: { current: { ...data, groupKey: "nested" } } });
  const collisions = sameGroupCollision({ active, droppableContainers: containers,
    droppableRects: new Map([["tab:a", { top: 0, bottom: 100 }], ["tab:b", { top: 100, bottom: 130 }],
      ["tab:c", { top: 130, bottom: 160 }], ["child", { top: 0, bottom: 20 }]]),
    pointerCoordinates: { x: 20, y: 25 },
  } as unknown as Parameters<typeof sameGroupCollision>[0]);
  expect(collisions.map(collision => collision.id)).toEqual(["tab:a"]);
  const reorderTabs = vi.fn();
  commitSidebarDrag({ active, over: containers[0] } as unknown as Parameters<typeof commitSidebarDrag>[0],
    { reorderTabs, reorderDisplay: vi.fn(), reorderWorkspaces: vi.fn() });
  expect(reorderTabs).toHaveBeenCalledWith({ workspaceId: "workspace", orderedTabIds: ["c", "a", "b"] });
});

