import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { ChatMessageRow, type MessageRowHandlers } from "./ChatMessageRow";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function render(selected: boolean, onDelete?: () => void) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const handlers: MessageRowHandlers = {
    clipFor: () => ({ id: "bubble", role: "user", author: "You", source: "opaque" }),
    partsFor: () => [], selectedFor: () => selected, cursorFor: () => true, joinNextFor: () => false,
    select: () => {}, rowMouseDown: () => {}, toggle: () => {}, range: () => {}, endRange: () => {}, quote: () => {}, ask: () => {},
  };
  await act(async () => root.render(<ChatMessageRow msg={{ id: "bubble", type: "user" }} ariaLabel="User message"
    selected={selected} joinNext={false} selecting={selected} cursor handlers={handlers} onDelete={onDelete}><p>opaque</p></ChatMessageRow>));
  return { host, row: host.querySelector<HTMLElement>("[data-message-id]")!, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}

it("context Delete invokes its handler once", async () => {
  const deleted = vi.fn();
  const view = await render(false, deleted);
  try {
    await act(async () => view.row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })));
    const action = view.host.querySelector<HTMLButtonElement>('[role="menuitem"][aria-label="Delete"]');
    expect(action).not.toBeNull();
    await act(async () => action!.click());
    expect(deleted).toHaveBeenCalledTimes(1);
  } finally { await view.close(); }
});

it("Delete key acts only on a selected bubble", async () => {
  const deleted = vi.fn();
  const selected = await render(true, deleted);
  try {
    await act(async () => selected.row.dispatchEvent(new KeyboardEvent("keydown", { code: "Delete", bubbles: true, cancelable: true })));
    expect(deleted).toHaveBeenCalledTimes(1);
  } finally { await selected.close(); }
  const unselected = await render(false, deleted);
  try {
    await act(async () => unselected.row.dispatchEvent(new KeyboardEvent("keydown", { code: "Delete", bubbles: true })));
    expect(deleted).toHaveBeenCalledTimes(1);
  } finally { await unselected.close(); }
});

it("a delivered row has no Delete action", async () => {
  const view = await render(true);
  try {
    await act(async () => { view.row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true })); view.row.dispatchEvent(new KeyboardEvent("keydown", { code: "Delete", bubbles: true })); });
    expect(view.host.querySelector('[role="menuitem"][aria-label="Delete"]')).toBeNull();
  } finally { await view.close(); }
});
