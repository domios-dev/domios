import assert from "node:assert/strict";
import { ChatScrollMemory, chatScrollBookmark, chatScrollRestoreTarget, initialChatScrollAnchor, decodeChatScrollMemory } from "./chatScrollMemory";
import { resolveChatAnchorTop } from "./chatAnchorModel";

const memory = new ChatScrollMemory(3);
const anchor = { id: "old-render", key: "source", ordinal: 1, offset: 37 };
memory.save("tab-a:session", { mode: "detached", scrollTop: 963, anchor });
memory.save("tab-b:session", { mode: "pinned", scrollTop: 1000 });
anchor.offset = 99;
const bookmark = chatScrollBookmark(memory.read("tab-a:session"));
assert.deepEqual(initialChatScrollAnchor(null), { pinned: true, top: 0, anchor: null });
assert.deepEqual(initialChatScrollAnchor(memory.read("tab-b:session")), { pinned: true, top: 0, anchor: null });
assert.deepEqual(initialChatScrollAnchor(memory.read("tab-a:session")), {
  pinned: false, top: 963, anchor: { key: "source", ordinal: 1, offset: 37 },
});
assert.deepEqual(initialChatScrollAnchor({ mode: "detached", scrollTop: 600 }), { pinned: true, top: 0, anchor: null });
assert.deepEqual(bookmark, { mode: "detached", anchor: { key: "source", ordinal: 1, offset: 37 } });
assert.deepEqual(chatScrollRestoreTarget(chatScrollBookmark(memory.read("tab-b:session")), null, 2000), { mode: "pinned", top: 2000 });
assert.equal(bookmark.mode, "detached");
if (bookmark.mode !== "detached") throw new Error("missing bookmark");
const top = resolveChatAnchorTop([
  { id: "old-render", key: "unrelated", ordinal: 0, top: 50 },
  { id: "new-render", key: "source", ordinal: 1, top: 1200 },
], bookmark.anchor);
assert.deepEqual(chatScrollRestoreTarget(bookmark, top, 2000), { mode: "detached", top: 1163 });
for (const unavailable of [null, -1, 2001, Number.NaN]) {
  assert.deepEqual(chatScrollRestoreTarget(bookmark, unavailable, 2000), { mode: "pinned", top: 2000 });
}
assert.deepEqual(chatScrollBookmark(memory.read("tab-a:other-session")), { mode: "pinned" });
assert.deepEqual(chatScrollBookmark({ mode: "detached", scrollTop: 600 }), { mode: "pinned" });
memory.clear("tab-a:session");
assert.equal(memory.read("tab-a:session"), null);
for (const key of ["c", "d", "e"]) memory.save(key, { mode: "pinned", scrollTop: 0 });
assert.equal(memory.read("tab-b:session"), null);
const stored = new Map<string, string>();
const storage = { getItem: (key: string) => stored.get(key) ?? null,
  setItem: (key: string, value: string) => { stored.set(key, value); } };
const persistence = { storage, key: "daemon:peer:page" };
const saved = new ChatScrollMemory(3, persistence);
const reading = { mode: "detached" as const, scrollTop: 2034,
  anchor: { key: "own-source-message", ordinal: 0, offset: -43 } };
saved.save("own-tab:own-session", reading);
assert.deepEqual(new ChatScrollMemory(3, persistence).read("own-tab:own-session"), reading);
assert.equal(new ChatScrollMemory(3, { storage, key: "other-daemon" }).read("own-tab:own-session"), null);
assert.equal(new ChatScrollMemory(3, persistence).read("own-tab:other-session"), null);
saved.clear("own-tab:own-session");
assert.equal(new ChatScrollMemory(3, persistence).read("own-tab:own-session"), null);
for (const key of ["a", "b", "c", "d"]) saved.save(key, reading);
assert.equal(new ChatScrollMemory(3, persistence).read("a"), null);
for (const raw of ["broken", "{}", '[null,["a",null],["b",{"mode":"detached","scrollTop":-1}]]']) {
  assert.deepEqual(decodeChatScrollMemory(raw), []);
}
const unavailable = new ChatScrollMemory(3, { key: "blocked", storage: {
  getItem: () => { throw new Error("unavailable"); }, setItem: () => { throw new Error("full"); },
} });
unavailable.save("own-tab:own-session", reading);
assert.deepEqual(unavailable.read("own-tab:own-session"), reading);
console.log("chatScrollMemory checks passed");
