import assert from "node:assert/strict";
import { ChatScrollMemory, chatScrollBookmark, chatScrollRestoreTarget } from "./chatScrollMemory";
import { resolveChatAnchorTop } from "./chatAnchorModel";

const memory = new ChatScrollMemory(3);
const anchor = { id: "old-render", key: "source", ordinal: 1, offset: 37 };
memory.save("tab-a:session", { mode: "detached", scrollTop: 963, anchor });
memory.save("tab-b:session", { mode: "pinned", scrollTop: 1000 });
anchor.offset = 99;
const bookmark = chatScrollBookmark(memory.read("tab-a:session"));
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
console.log("chatScrollMemory checks passed");
