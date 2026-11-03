import { describe, expect, it } from "vitest";
import { composerDraftRecords, editComposerDraft, readComposerDraft, restoreComposerDrafts } from "./drafts";
import { DraftPersistence } from "./draftPersistence";
import type { ClientCheckpoint } from "./checkpoint";

const empty: ClientCheckpoint = { activeWorkspaceId: null, activeTabId: null, drafts: {}, scroll: {} };
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}
function memoryStorage() {
  const values = new Map<string, string>();
  return { values, read: async () => new Map(values), write: async (scope: string, value: string | null) => { if (value === null) values.delete(scope); else values.set(scope, value); } };
}

describe("asynchronous composer drafts", () => {
  it("does not restart a failed write when its error subscriber throws", async () => {
    let attempts = 0;
    const drafts = new DraftPersistence({ read: async () => new Map(), write: async () => {
      attempts += 1;
      throw new Error("unavailable");
    } });
    await drafts.initialize();
    const stop = drafts.subscribe(() => {
      if (drafts.error("t")) throw new Error("error observer failed");
    });
    drafts.edit("t", "preserved");
    await expect(drafts.flush()).rejects.toThrow("error observer failed");
    stop();
    expect(attempts).toBe(1);
    expect(drafts.records.get("t")).toBe("preserved");
    await expect(drafts.flush()).rejects.toThrow("unavailable");
    expect(attempts).toBe(2);
  });

  it("restores text, capsules and reply identity in a fresh page and keeps sent text cleared", async () => {
    const storage = memoryStorage();
    const first = new DraftPersistence(storage);
    first.edit("tab-session-chat:t", "unsent [image1]");
    first.edit("tab-session-chat-capsules:t", '[{"label":"image1","kind":"image","path":"/uploads/image.png"}]');
    first.edit("tab-session-chat-reply:t", "opaque-reply-record");
    await first.flush();
    const reloaded = new DraftPersistence(storage);
    await reloaded.initialize();
    const restored = restoreComposerDrafts(empty, reloaded.records);
    expect(restored.drafts["tab-session-chat:t"]).toMatchObject({ text: "unsent [image1]", attachmentIds: ["image1"],
      capsules: { image1: { path: "/uploads/image.png" } } });
    expect(readComposerDraft(restored.drafts, "tab-session-chat-reply:t")).toBe("opaque-reply-record");
    reloaded.edit("tab-session-chat:t", "");
    reloaded.edit("tab-session-chat-capsules:t", "[]");
    reloaded.edit("tab-session-chat-reply:t", "");
    await reloaded.flush();
    const afterSend = new DraftPersistence(storage);
    await afterSend.initialize();
    expect(restoreComposerDrafts(empty, afterSend.records).drafts["tab-session-chat:t"]).toMatchObject({ text: "", attachmentIds: [], capsules: {} });
    expect(afterSend.records.get("tab-session-chat-reply:t")).toBe("");
  });

  it("keeps one in-flight write and the latest pending value per scope, including a send clear", async () => {
    const entered = deferred();
    const release = deferred();
    const writes: [string, string | null][] = [];
    const storage = memoryStorage();
    const drafts = new DraftPersistence({ ...storage, write: async (scope, value) => {
      writes.push([scope, value]);
      if (scope === "a" && value === "first") { entered.resolve(); await release.promise; }
      await storage.write(scope, value);
    } });
    drafts.edit("a", "first");
    expect(writes).toEqual([]);
    await entered.promise;
    for (let index = 0; index < 100; index++) drafts.edit("a", `pending-${index}`);
    drafts.edit("b", "independent");
    drafts.edit("a", "");
    const flushed = drafts.flush();
    expect(drafts.records.get("a")).toBe("");
    release.resolve();
    await flushed;
    expect(writes.filter(([scope]) => scope === "a")).toEqual([["a", "first"], ["a", ""]]);
    expect(storage.values.get("a")).toBe("");
    expect(storage.values.get("b")).toBe("independent");
  });

  it("surfaces failed persistence, retains live input, and explicitly flushes the newest value", async () => {
    const storage = memoryStorage();
    let refused = true;
    const drafts = new DraftPersistence({ ...storage, write: async (scope, value) => {
      if (refused) throw new Error("quota");
      await storage.write(scope, value);
    } });
    drafts.edit("t", "still live");
    await expect(drafts.flush()).rejects.toThrow("quota");
    expect(drafts.records.get("t")).toBe("still live");
    expect(drafts.error("t")?.message).toBe("quota");
    drafts.edit("t", "");
    refused = false;
    await drafts.flush();
    expect(drafts.error("t")).toBeUndefined();
    expect(storage.values.get("t")).toBe("");
  });

  it("does not overwrite an edit or send clear when asynchronous restore finishes", async () => {
    const loaded = deferred();
    const drafts = new DraftPersistence({ read: async () => {
      await loaded.promise;
      return new Map([["a", "old unsent"], ["b", "saved"]]);
    }, write: async () => {} });
    const initializing = drafts.initialize();
    drafts.edit("a", "");
    loaded.resolve();
    await initializing;
    await drafts.flush();
    expect([...drafts.records]).toEqual([["a", ""], ["b", "saved"]]);
  });

  it("reports a failed restore without pretending no saved drafts exist", async () => {
    const drafts = new DraftPersistence({ read: async () => { throw new Error("unavailable"); }, write: async () => {} });
    await drafts.initialize();
    expect(drafts.error("t")?.message).toBe("unavailable");
    await expect(drafts.flush()).rejects.toThrow("unavailable");
  });

  it("preserves checkpoint precedence and empty tombstones at the reload handoff", () => {
    const checkpoint = structuredClone(empty);
    editComposerDraft(checkpoint.drafts, "tab-session-chat:t", "");
    editComposerDraft(checkpoint.drafts, "tab-session-chat-capsules:t", "[]");
    const restored = restoreComposerDrafts(checkpoint, new Map([["tab-session-chat:t", "stale"], ["tab-session-chat:other", "fallback"]]));
    expect(restored.drafts["tab-session-chat:t"].text).toBe("");
    expect(restored.drafts["tab-session-chat:other"].text).toBe("fallback");
    expect(composerDraftRecords(restored).get("tab-session-chat-capsules:t")).toBe("");
  });
});

it("prunes orphaned async records after restore and preserves live and unscoped drafts", async () => {
  const values = new Map([["tab-session-chat:dead", "old"], ["tab-session-chat-capsules:dead", "[]"],
    ["tab-session-chat-reply:live", "reply"], ["unscoped", "kept"]]);
  const persistence = new DraftPersistence({ read: async () => new Map(values), write: async (scope, value) => {
    if (value === null) values.delete(scope); else values.set(scope, value);
  } });
  await persistence.prune(new Set());
  expect(values.size).toBe(4);
  await persistence.prune(new Set(["live"]));
  await persistence.flush();
  expect([...values]).toEqual([["tab-session-chat-reply:live", "reply"], ["unscoped", "kept"]]);
});

it("starts the newest edit even when it arrives between write completion and queue cleanup", async () => {
  const entered = deferred();
  const completed = deferred();
  const writes: string[] = [];
  const drafts = new DraftPersistence({ read: async () => new Map(), write: (_scope, value) => {
    writes.push(value ?? "deleted");
    entered.resolve();
    return completed.promise;
  } });
  drafts.edit("t", "first");
  await entered.promise;
  completed.resolve();
  await completed.promise;
  drafts.edit("t", "newest");
  await drafts.flush();
  expect(writes).toEqual(["first", "newest"]);
});

it("uses the newest live-tab snapshot when pruning waits for disk restore", async () => {
  const restored = deferred();
  const values = new Map([["tab-session-chat:new", "keep"], ["tab-session-chat:dead", "remove"]]);
  const drafts = new DraftPersistence({ read: async () => { await restored.promise; return new Map(values); },
    write: async (scope, value) => { if (value === null) values.delete(scope); else values.set(scope, value); } });
  const oldSnapshot = drafts.prune(new Set(["old"]));
  const newSnapshot = drafts.prune(new Set(["new"]));
  restored.resolve();
  await Promise.all([oldSnapshot, newSnapshot]);
  await drafts.flush();
  expect([...values]).toEqual([["tab-session-chat:new", "keep"]]);
});
