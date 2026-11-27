import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "./bridge";
import { frontendSignals } from "./frontendUpdate/appState";
import { DraftPersistence, REPEATED_DRAFT_FAILURE_THRESHOLD } from "./frontendUpdate/draftPersistence";
import { createDraftStorage } from "./frontendUpdate/draftStorage";
import { DraftPersistenceContext, usePersistentDraft } from "./usePersistentDraft";
import { ChatComposer } from "../components/Terminal/ChatComposer";
import { createMuxStateStore } from "../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../store/multiplexer/context";
import type { ExtendedState } from "../store/multiplexer/tree";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  __setInvokeForTest(null);
  __setListenForTest(null);
});

function harness(persistence: DraftPersistence) {
  frontendSignals.hydrate({ activeWorkspaceId: null, activeTabId: null, drafts: {}, scroll: {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const handle = { value: "", saveUnavailable: false, edit: (_value: string) => {} };
  function Field({ scope }: { scope: string }) {
    const [value, edit, status] = usePersistentDraft(scope, persistence);
    handle.value = value;
    handle.saveUnavailable = status.saveUnavailable;
    handle.edit = edit;
    return <textarea aria-label="Draft" value={value} onChange={event => edit(event.target.value)} />;
  }
  const render = (scope: string) => act(async () => { root.render(<Field scope={scope} />); });
  cleanups.push(async () => { await act(async () => root.unmount()); host.remove(); });
  return { handle, render };
}

function closingFakeIdb() {
  const rows = new Map<string, { client: string; scope: string; value: string }>();
  const handles: Array<{ closed: boolean; closing: boolean; failAll: boolean }> = [];
  let createdStore = false;
  let failAllWrites = false;

  const factory: Pick<IDBFactory, "open"> = {
    open() {
      const request = {} as IDBOpenDBRequest;
      const state = { closed: false, closing: false, failAll: false };
      handles.push(state);
      const db = {
        objectStoreNames: { contains: (name: string) => createdStore && name === "drafts" },
        createObjectStore: () => { createdStore = true; return { createIndex: () => ({}) }; },
        close: () => { state.closing = true; state.closed = true; },
        transaction(_store: string, mode: IDBTransactionMode) {
          if (state.closing || state.closed) {
            throw new DOMException(
              "Failed to execute 'transaction' on 'IDBDatabase': The database connection is closing.",
              "InvalidStateError",
            );
          }
          const transaction = { error: null as Error | null } as unknown as IDBTransaction;
          const actions: Array<() => void> = [];
          let scheduled = false;
          const finish = () => {
            if (scheduled) return;
            scheduled = true;
            queueMicrotask(() => {
              if (mode === "readwrite" && (failAllWrites || state.failAll)) {
                (transaction as { error: Error }).error = new DOMException("Disk full", "QuotaExceededError");
                transaction.onabort?.call(transaction, new Event("abort"));
                return;
              }
              for (const run of actions) run();
              transaction.oncomplete?.call(transaction, new Event("complete"));
            });
          };
          (transaction as unknown as { objectStore: () => unknown }).objectStore = () => ({
            put(record: { client: string; scope: string; value: string }) {
              actions.push(() => { rows.set(`${record.client}\u0000${record.scope}`, record); });
              finish();
              return {};
            },
            delete([client, scope]: [string, string]) {
              actions.push(() => { rows.delete(`${client}\u0000${scope}`); });
              finish();
              return {};
            },
            index: () => ({
              getAll(client: string) {
                const req = { result: [] as Array<{ client: string; scope: string; value: string }> };
                actions.push(() => { req.result = [...rows.values()].filter(row => row.client === client); });
                finish();
                return req;
              },
            }),
          });
          return transaction;
        },
        onversionchange: null,
        onclose: null,
      } as unknown as IDBDatabase;
      (request as { result: IDBDatabase }).result = db;
      queueMicrotask(() => {
        if (!createdStore) request.onupgradeneeded?.call(request, { oldVersion: 0, newVersion: 1 } as IDBVersionChangeEvent);
        request.onsuccess?.call(request, new Event("success"));
      });
      return request;
    },
  };
  return {
    factory,
    handles,
    rows,
    setFailAllWrites(next: boolean) { failAllWrites = next; },
  };
}

async function mountComposer(tabId: string, persistence: DraftPersistence) {
  frontendSignals.hydrate({ activeWorkspaceId: null, activeTabId: null, drafts: {}, scroll: {} });
  __setInvokeForTest((async (command: string) => (command === "delayed_send_list" ? [] : {})) as never);
  __setListenForTest((async () => () => {}) as never);
  const state: ExtendedState = {
    workspaces: [],
    tabs: { [tabId]: { id: tabId, pty_id: null, tab_type: "Terminal", title: "Drafts", cwd: null } },
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
  const showComposerError = vi.fn();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <DraftPersistenceContext value={persistence}>
        <MuxStateStoreContext.Provider value={createMuxStateStore(() => state)}>
          <ChatComposer
            tabId={tabId}
            tabProvider={null}
            pendingPrompt={null}
            attachments={[]}
            addAttachments={() => {}}
            removeAttachment={() => {}}
            clearAttachments={() => {}}
            handlePasteEvent={() => {}}
            onWillSendMessage={() => {}}
            addOptimistic={() => "opt"}
            updateOptimistic={() => {}}
            observeChatSendStatus={() => {}}
            showComposerError={showComposerError}
          />
        </MuxStateStoreContext.Provider>
      </DraftPersistenceContext>,
    );
  });
  cleanups.push(async () => { await act(async () => root.unmount()); host.remove(); });
  const textarea = host.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"]')!;
  const type = async (value: string) => {
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, value);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
      await persistence.flush().catch(() => {});
    });
  };
  return { host, textarea, showComposerError, type };
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

it("keeps drafts live on transient failure, retries on next change, and flags only repeated failures", async () => {
  let refused = true;
  const saved = new Map<string, string>();
  const persistence = new DraftPersistence({
    read: async () => new Map(),
    write: async (scope, value) => {
      if (refused) throw new Error("quota");
      if (value === null) saved.delete(scope);
      else saved.set(scope, value);
    },
  });
  const field = harness(persistence);
  await field.render("quota");

  for (let step = 1; step < REPEATED_DRAFT_FAILURE_THRESHOLD; step += 1) {
    await act(async () => { field.handle.edit(`attempt-${step}`); await persistence.flush().catch(() => {}); });
    expect(field.handle.value).toBe(`attempt-${step}`);
    expect(field.handle.saveUnavailable).toBe(false);
  }

  await act(async () => { field.handle.edit("attempt-repeated"); await persistence.flush().catch(() => {}); });
  expect(field.handle.value).toBe("attempt-repeated");
  expect(field.handle.saveUnavailable).toBe(true);

  refused = false;
  await act(async () => { field.handle.edit("recovered"); await persistence.flush(); });
  expect(field.handle.value).toBe("recovered");
  expect(field.handle.saveUnavailable).toBe(false);
  expect(saved.get("quota")).toBe("recovered");
});

it("keeps edits live when storage is unavailable while explicit flush still rejects", async () => {
  const unavailable = async () => { throw new Error("Draft storage is unavailable"); };
  const persistence = new DraftPersistence({ read: unavailable, write: unavailable });
  const field = harness(persistence);
  await field.render("unavailable");
  expect(field.handle.saveUnavailable).toBe(false);
  await act(async () => { field.handle.edit("unsaved but editable"); });
  expect(field.handle.value).toBe("unsaved but editable");
  await act(async () => { await expect(persistence.flush()).rejects.toThrow("Draft storage is unavailable"); });
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

it("survives a closing IndexedDB handle in ChatComposer without raising a Chat error toast", async () => {
  const fake = closingFakeIdb();
  const persistence = new DraftPersistence(createDraftStorage(fake.factory));
  const composer = await mountComposer("closing-tab", persistence);

  await composer.type("first draft");
  expect(fake.handles).toHaveLength(1);
  expect(composer.showComposerError).not.toHaveBeenCalled();

  fake.handles[0].closing = true;
  await composer.type("first draft updated");

  expect(fake.handles).toHaveLength(2);
  expect(fake.handles[0].closed).toBe(true);
  expect(composer.textarea.value).toBe("first draft updated");
  expect(composer.showComposerError).not.toHaveBeenCalled();
  expect(composer.host.querySelector('[data-testid="draft-save-hint"]')).toBeNull();
  expect(composer.host.querySelector('[role="alert"], [role="dialog"]')).toBeNull();
});

it("never raises a blocking Chat error toast on repeated draft save failures and shows a quiet hint only until recovery", async () => {
  const fake = closingFakeIdb();
  fake.setFailAllWrites(true);
  const persistence = new DraftPersistence(createDraftStorage(fake.factory));
  const composer = await mountComposer("failing-tab", persistence);

  await composer.type("one");
  await composer.type("two");
  expect(composer.showComposerError).not.toHaveBeenCalled();
  expect(composer.textarea.getAttribute("aria-invalid")).toBe("false");
  expect(composer.host.querySelector('[data-testid="draft-save-hint"]')).toBeNull();

  await composer.type("three");
  expect(composer.showComposerError).not.toHaveBeenCalled();
  expect(composer.textarea.value).toBe("three");
  expect(composer.textarea.getAttribute("aria-invalid")).toBe("false");
  expect(composer.host.querySelector('[role="alert"], [role="dialog"]')).toBeNull();
  const hint = composer.host.querySelector('[data-testid="draft-save-hint"]');
  expect(hint?.getAttribute("role")).toBe("note");

  fake.setFailAllWrites(false);
  await composer.type("four");
  expect(composer.showComposerError).not.toHaveBeenCalled();
  expect(composer.host.querySelector('[data-testid="draft-save-hint"]')).toBeNull();
});
