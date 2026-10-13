import assert from "node:assert/strict";
import { editComposerDraft, restoreComposerDrafts, readComposerDraft } from "./drafts";
import { checkpointWrite, type ClientCheckpoint } from "./checkpoint";
import { createFrontendSignals } from "./signals";
import { COMPOSER_DRAFT_PREFIX } from "../composerDraftGc";

function storage(entries: Record<string, string> = {}) {
  const values = new Map(Object.entries(entries));
  return {
    get length() { return values.size; },
    key: (index: number) => [...values.keys()][index] ?? null,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
  };
}
const empty: ClientCheckpoint = { activeWorkspaceId: null, activeTabId: null, drafts: {}, scroll: {} };
const safety = { focused: false, idle: true, typing: false, composing: false, modalOpen: false, pendingUploads: 0 };
const signals = createFrontendSignals(empty, safety, value => value);
const edited = signals.checkpoint();
editComposerDraft(edited.drafts, "tab-session-chat:t", "last unsent edit");
editComposerDraft(edited.drafts, "tab-session-chat-capsules:t", '[{"label":"image1","kind":"image","path":"/uploads/image.png"}]');
signals.edit(edited);
const captured = checkpointWrite(1, signals.checkpoint());
assert.deepEqual(captured.payload.drafts, edited.drafts);
const freshLocal = storage();
const fresh = createFrontendSignals(empty, safety, value => restoreComposerDrafts(value, freshLocal));
fresh.hydrate(captured.payload);
assert.equal(readComposerDraft(fresh.checkpoint().drafts, freshLocal, "tab-session-chat:t"), "last unsent edit");
assert.deepEqual(fresh.checkpoint().drafts, edited.drafts);
assert.deepEqual(fresh.checkpoint().drafts["tab-session-chat:t"].attachmentIds, ["image1"]);
assert.equal(readComposerDraft(fresh.checkpoint().drafts, freshLocal, "tab-session-chat-capsules:t"), '[{"label":"image1","kind":"image","path":"/uploads/image.png"}]');
assert.equal(freshLocal.getItem(COMPOSER_DRAFT_PREFIX + "tab-session-chat:t"), "last unsent edit");
const fallback = storage({
  [COMPOSER_DRAFT_PREFIX + "tab-session-chat:t"]: "stale local",
  [COMPOSER_DRAFT_PREFIX + "tab-session-chat:other"]: "local fallback",
  [COMPOSER_DRAFT_PREFIX + "tab-session-chat-capsules:other"]: '[{"label":"file1","kind":"file","path":"/uploads/report.pdf"}]',
  "unrelated-credential": "opaque",
});
const restored = restoreComposerDrafts(captured.payload, fallback);
assert.equal(restored.drafts["tab-session-chat:t"].text, "last unsent edit");
assert.equal(restored.drafts["tab-session-chat:other"].text, "local fallback");
assert.equal(Object.keys(restored.drafts).length, 2);
assert.deepEqual(restored.drafts["tab-session-chat:other"].attachmentIds, ["file1"]);
assert.equal(restored.drafts["tab-session-chat:other"].capsules?.file1.path, "/uploads/report.pdf");
const cleared = structuredClone(captured.payload);
cleared.drafts["tab-session-chat:t"].text = "";
assert.equal(restoreComposerDrafts(cleared, fallback).drafts["tab-session-chat:t"].text, "");
assert.equal(fallback.getItem(COMPOSER_DRAFT_PREFIX + "tab-session-chat:t"), null);
console.log("draft checkpoint model: capture, empty-local restore, precedence, fallback, and clearing passed");
