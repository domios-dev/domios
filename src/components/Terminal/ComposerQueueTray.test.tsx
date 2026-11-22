import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { ComposerQueueTray } from "./ComposerQueueTray";
import { applyQueuedSendEdit, composerSendFeedback, type QueuedSendItem } from "./composerQueueModel";
import type { ChatSendStatus } from "./chatSendStatus";

const change = vi.hoisted(() => vi.fn());
vi.mock("./chatSendStatus", () => ({ changeComposerSendQueue: change }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const item: QueuedSendItem = { sendId: "send", correlationId: "original-token", version: 2, preview: "Original [image]" };
const held: ChatSendStatus = { send_id: "send", tab_id: "tab", correlationId: item.correlationId,
  ledger_id: "ledger", sequence: 1, retained_from: 1, error: null, state: "held" };

test("queue feedback counts editable items and shows sending from actual admissions or handoffs", () => {
  const queue = { items: [item], claimed: [], edits: [] };
  const local = { id: "local", content: "opaque", sentAtMs: 1, correlationId: item.correlationId };
  expect(composerSendFeedback([local], [], { ...queue, items: [] })).toEqual({ sending: true, items: [], unsettled: true });
  expect(composerSendFeedback([], [held], queue)).toEqual({ sending: false, items: [item], unsettled: true });
  expect(composerSendFeedback([], [held], { ...queue, items: [], claimed: ["send"] })).toEqual({ sending: true, items: [], unsettled: true });
  expect(composerSendFeedback([], [{ ...held, state: "written" }], queue)).toEqual({ sending: true, items: [], unsettled: true });
  for (const state of ["submit_unconfirmed", "delivered"] as const) {
    expect(composerSendFeedback([], [{ ...held, state }], queue)).toEqual({ sending: false, items: [], unsettled: false });
  }
  const addition = { ...item, appended: "\n\nnew line" };
  const revised = applyQueuedSendEdit({ ...local, status: held }, addition);
  expect(revised.content).toBe("opaque\n\nnew line");
  expect(applyQueuedSendEdit(revised, addition)).toBe(revised);
  expect(applyQueuedSendEdit({ ...local, status: { ...held, send_id: "other" } }, addition).content).toBe("opaque");
});

test("the tray submits the observed version and surfaces a refused claim race", async () => {
  change.mockResolvedValue({ outcome: "unavailable" });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ComposerQueueTray tabId="tab" items={[item]} editable />));
    const trigger = host.querySelector("button")!;
    expect(trigger.textContent).toBe("Queued: 1");
    await act(async () => trigger.click());
    const dialog = document.querySelector('[role="dialog"][aria-label="Queued messages"]')!;
    const append = dialog.querySelector<HTMLButtonElement>('button[aria-label="Append to queued message 1"]')!;
    await act(async () => append.click());
    const editor = dialog.querySelector("textarea")!;
    expect(document.activeElement).toBe(editor);
    const value = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")!.set!;
    await act(async () => {
      value.call(editor, "a detail");
      editor.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => dialog.querySelector<HTMLButtonElement>('button[type="submit"]')!.click());
    const key = { sendId: item.sendId, correlationId: item.correlationId, version: item.version };
    expect(change).toHaveBeenLastCalledWith("tab", { ...key, action: "append", text: "a detail" });
    expect(host.querySelector('[role="alert"]')?.textContent).toMatch(/left the editable queue or changed/);
    await act(async () => dialog.querySelector<HTMLButtonElement>('button[aria-label="Cancel queued message 1"]')!.click());
    expect(change).toHaveBeenLastCalledWith("tab", { ...key, action: "cancel" });
    await act(async () => root.render(<ComposerQueueTray tabId="tab" items={[]} editable />));
    expect(document.querySelector('[aria-label="Queued messages"]')).toBeNull();
    expect(host.textContent).not.toContain("Sending…");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
