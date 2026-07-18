import { isChatViewportAtBottom, shouldRestoreChatViewport } from "./chatViewport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const key = "pane:session";

assert(!shouldRestoreChatViewport(true, false, 0, key, ""), "empty activation waits for messages");
assert(!shouldRestoreChatViewport(true, true, 1, key, ""), "loading activation waits for the session");
assert(shouldRestoreChatViewport(true, false, 1, key, ""), "received messages restore the bottom viewport");
assert(!shouldRestoreChatViewport(true, false, 1, key, key), "one activation restores only once");
assert(!shouldRestoreChatViewport(false, false, 1, key, ""), "hidden chat never scrolls");

assert(isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 600, clientHeight: 400 }), "exact bottom follows messages");
assert(isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 598, clientHeight: 400 }), "subpixel bottom gap follows messages");
assert(!isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 597, clientHeight: 400 }), "manual scroll detaches immediately");

console.log("chat viewport activation checks passed");
