import assert from "node:assert/strict";
import test from "node:test";
import { COMPOSER_DRAFT_PREFIX, createComposerDraftPruner } from "./composerDraftGc";

test("background snapshots do not rescan storage; changed tab membership still reclaims only closed drafts", () => {
  const key = (id: string) => `${COMPOSER_DRAFT_PREFIX}tab-session-chat:${id}`;
  const drafts = new Map([[key("one"), "local edit"], [key("two"), "second draft"], [key("orphan"), "old draft"]]);
  let reads = 0;
  const prune = createComposerDraftPruner({
    get length() { reads++; return drafts.size; },
    key(index) { reads++; return [...drafts.keys()][index] ?? null; },
    getItem(id) { reads++; return drafts.get(id) ?? null; },
    removeItem(id) { drafts.delete(id); },
  });
  assert.deepEqual(prune(new Set()), []);
  assert.equal(reads, 0, "pre-hydration never accesses storage");
  assert.deepEqual(prune(new Set(["one", "two"])), [key("orphan")]);
  const initialReads = reads;
  for (let snapshot = 0; snapshot < 100; snapshot++) prune(new Set(["two", "one"]));
  assert.equal(reads, initialReads, "unrelated runtime activity must not enumerate localStorage");
  assert.deepEqual(prune(new Set(["one"])), [key("two")]);
  assert.equal(drafts.get(key("one")), "local edit");
  assert(reads > initialReads);
  const changedReads = reads;
  prune(new Set());
  prune(new Set(["one"]));
  assert.equal(reads, changedReads, "transient empty state must not reset the last authoritative membership");
  assert.deepEqual(prune(new Set(["other"])), [key("one")]);
});

test("failed storage access is retried on the next state event", () => {
  let unavailable = true;
  let reads = 0;
  const prune = createComposerDraftPruner({
    get length() { reads++; if (unavailable) throw new Error("storage unavailable"); return 0; },
    key() { return null; }, getItem() { return null; }, removeItem() {},
  });
  assert.throws(() => prune(new Set(["one"])));
  unavailable = false;
  prune(new Set(["one"]));
  assert.equal(reads, 2);
});
