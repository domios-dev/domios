import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { frontendSignals } from "./frontendUpdate/appState";
import { DraftPersistence } from "./frontendUpdate/draftPersistence";
import { usePersistentDraft } from "./usePersistentDraft";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0)) await cleanup(); });

function harness(persistence: DraftPersistence) {
  frontendSignals.hydrate({ activeWorkspaceId: null, activeTabId: null, drafts: {}, scroll: {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onError = vi.fn();
  const handle = { value: "", edit: (_value: string) => {} };
  function Field({ scope }: { scope: string }) {
    const [value, edit] = usePersistentDraft(scope, onError, persistence);
    handle.value = value;
    handle.edit = edit;
    return <textarea aria-label="Draft" value={value} onChange={event => edit(event.target.value)} />;
  }
  const render = (scope: string) => act(async () => { root.render(<Field scope={scope} />); });
  cleanups.push(async () => { await act(async () => root.unmount()); host.remove(); });
  return { handle, render, onError };
}

it("keeps rapid edits live without localStorage writes or a checkpoint clone", async () => {
  const values = new Map<string, string>();
  const persistence = new DraftPersistence({ read: async () => new Map(), write: async (scope, value) => { if (value === null) values.delete(scope); else values.set(scope, value); } });
  const field = harness(persistence);
  const clone = vi.spyOn(frontendSignals, "checkpoint");
  const save = vi.spyOn(window.localStorage, "setItem");
  try {
    await field.render("rapid");
    await act(async () => { field.handle.edit("a"); field.handle.edit("ab"); field.handle.edit("abc"); await persistence.flush(); });
    expect(field.handle.value).toBe("abc");
    expect(frontendSignals.checkpointView().drafts.rapid.text).toBe("abc");
    expect(values.get("rapid")).toBe("abc");
    expect(clone).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
  } finally { clone.mockRestore(); save.mockRestore(); }
});

it("restores saved records and flushes each scope without leaking old text", async () => {
  const values = new Map([["a", "restored"]]);
  const persistence = new DraftPersistence({ read: async () => new Map(values), write: async (scope, value) => { if (value === null) values.delete(scope); else values.set(scope, value); } });
  const field = harness(persistence);
  await field.render("a");
  expect(field.handle.value).toBe("restored");
  await act(async () => field.handle.edit("alpha"));
  await field.render("b");
  expect(field.handle.value).toBe("");
  await act(async () => { field.handle.edit("beta"); window.dispatchEvent(new Event("blur")); await persistence.flush(); });
  expect(values.get("a")).toBe("alpha");
  expect(values.get("b")).toBe("beta");
  await act(async () => { field.handle.edit(""); window.dispatchEvent(new Event("pagehide")); await persistence.flush(); });
  expect(values.get("b")).toBe("");
});

it("surfaces persistence refusal to the composer while preserving its editable value", async () => {
  const persistence = new DraftPersistence({ read: async () => new Map(), write: async () => { throw new Error("quota"); } });
  const field = harness(persistence);
  await field.render("quota");
  await act(async () => { field.handle.edit("survives"); await persistence.flush().catch(() => {}); });
  expect(field.handle.value).toBe("survives");
  expect(field.onError).toHaveBeenCalledWith("Draft could not be saved: quota");
});

it("reports unavailable storage through the composer error callback and keeps edits live", async () => {
  const unavailable = async () => { throw new Error("Draft storage is unavailable"); };
  const persistence = new DraftPersistence({ read: unavailable, write: unavailable });
  const field = harness(persistence);
  await field.render("unavailable");
  expect(field.onError).toHaveBeenCalledWith("Draft could not be saved: Draft storage is unavailable");
  await act(async () => { field.handle.edit("unsaved but editable"); });
  expect(field.handle.value).toBe("unsaved but editable");
  await expect(persistence.flush()).rejects.toThrow("Draft storage is unavailable");
});

it("restores checkpoint text and capsule identity without replacing them with older disk records", async () => {
  const persistence = new DraftPersistence({ read: async () => new Map([["tab-session-chat:t", "old"]]), write: async () => {} });
  const field = harness(persistence);
  frontendSignals.hydrate({ activeWorkspaceId: null, activeTabId: null, drafts: {
    "tab-session-chat:t": { text: "new [image1]", attachmentIds: ["image1"], replyTo: null,
      capsules: { image1: { label: "image1", path: "/uploads/image.png", kind: "image" } } },
  }, scroll: {} });
  await field.render("tab-session-chat:t");
  expect(field.handle.value).toBe("new [image1]");
  await field.render("tab-session-chat-capsules:t");
  expect(JSON.parse(field.handle.value)).toEqual([{ label: "image1", path: "/uploads/image.png", kind: "image" }]);
});
