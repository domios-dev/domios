import { COMPOSER_DRAFT_PREFIX, draftKeyPaneId, pruneOrphanComposerDrafts } from "./composerDraftGc";

const failures: string[] = [];
function assert(cond: unknown, msg: string) {
  if (!cond) failures.push(msg);
}

class FakeStorage {
  private map = new Map<string, string>();
  get length() {
    return this.map.size;
  }
  key(i: number): string | null {
    return [...this.map.keys()][i] ?? null;
  }
  getItem(k: string): string | null {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    this.map.set(k, v);
  }
  removeItem(k: string) {
    this.map.delete(k);
  }
  keys(): string[] {
    return [...this.map.keys()];
  }
}

const chatKey = (id: string) => `${COMPOSER_DRAFT_PREFIX}pane-session-chat:${id}`;
const viewKey = (id: string) => `${COMPOSER_DRAFT_PREFIX}chat-view:${id}`;

// draftKeyPaneId
assert(draftKeyPaneId(chatKey("abcd1234")) === "abcd1234", "pane-session-chat pane id parsed");
assert(draftKeyPaneId(viewKey("deadbeef")) === "deadbeef", "chat-view Tab id parsed");
assert(draftKeyPaneId("codeterm:other:x:y") === null, "non-draft key rejected");
assert(
  draftKeyPaneId(`${COMPOSER_DRAFT_PREFIX}unknown-channel:abcd1234`) === null,
  "unknown channel not treated as pane-scoped",
);
assert(draftKeyPaneId(`${COMPOSER_DRAFT_PREFIX}pane-session-chat:`) === null, "empty Tab id rejected");

// prune: removes orphans, keeps live, ignores foreign keys
{
  const s = new FakeStorage();
  s.setItem(chatKey("live0001"), "keep me");
  s.setItem(chatKey("dead0002"), "continue working on the 1.7.2 stability plan");
  s.setItem(viewKey("dead0003"), "orphaned direct-chat draft");
  s.setItem(viewKey("live0001"), "keep me too");
  s.setItem("codeterm:unrelated", "not a draft");
  const removed = pruneOrphanComposerDrafts(s, new Set(["live0001"]));
  assert(removed.length === 2, `removes exactly the two orphans (got ${removed.length})`);
  assert(s.getItem(chatKey("dead0002")) === null, "orphan pane-session-chat draft removed");
  assert(s.getItem(viewKey("dead0003")) === null, "orphan chat-view draft removed");
  assert(s.getItem(chatKey("live0001")) === "keep me", "live pane-session-chat draft kept");
  assert(s.getItem(viewKey("live0001")) === "keep me too", "live chat-view draft kept");
  assert(s.getItem("codeterm:unrelated") === "not a draft", "unrelated key untouched");
}

// prune: empty live set is a no-op (pre-hydration guard, never wipes)
{
  const s = new FakeStorage();
  s.setItem(chatKey("any00001"), "would be wiped without the guard");
  const removed = pruneOrphanComposerDrafts(s, new Set());
  assert(removed.length === 0, "empty live set prunes nothing");
  assert(s.getItem(chatKey("any00001")) === "would be wiped without the guard", "draft survives pre-hydration");
}

if (failures.length > 0) {
  for (const f of failures) console.error("  ✗", f);
  console.error(`composerDraftGc: ${failures.length} check(s) failed`);
  process.exit(1);
}
console.log("composerDraftGc checks passed");
