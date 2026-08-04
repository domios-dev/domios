import {
  chatViewportModeAfterScroll,
  chatScrollTopAfterPrepend,
  clearPersistedChatScrollState,
  getPersistedChatScrollState,
  isChatViewportAtBottom,
  nextChatViewportMode,
  setPersistedChatScrollState,
  shouldRestoreChatViewport,
  snapChatViewportToBottom,
} from "./chatViewport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const key = "tab:session";

assert(!shouldRestoreChatViewport(true, false, 0, key, ""), "empty activation waits for messages");
assert(!shouldRestoreChatViewport(true, true, 1, key, ""), "loading activation waits for the session");
assert(shouldRestoreChatViewport(true, false, 1, key, ""), "received messages restore the viewport");
assert(!shouldRestoreChatViewport(true, false, 1, key, key), "one activation restores only once");
assert(!shouldRestoreChatViewport(false, false, 1, key, ""), "hidden chat never scrolls");

assert(isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 600, clientHeight: 400 }), "exact bottom is pinned");
assert(isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 599.75, clientHeight: 400 }), "rounding residue is bottom");
assert(!isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 599, clientHeight: 400 }), "one pixel away is detached");
assert(
  nextChatViewportMode({ scrollHeight: 1200, scrollTop: 600, clientHeight: 400 }) === "detached",
  "a viewport away from bottom stays detached",
);
assert(
  chatViewportModeAfterScroll("pinned", 600, { scrollHeight: 1004, scrollTop: 600, clientHeight: 400 }) === "pinned",
  "a viewport resize cannot detach a pinned live tail",
);
assert(
  chatViewportModeAfterScroll("pinned", 600, { scrollHeight: 1000, scrollTop: 599.9, clientHeight: 400 }) === "pinned",
  "fractional browser clamping cannot detach a pinned live tail",
);
assert(
  chatViewportModeAfterScroll("pinned", 600, { scrollHeight: 1000, scrollTop: 599, clientHeight: 400 }) === "detached",
  "meaningful upward movement detaches immediately",
);

const detachedViewport = { scrollHeight: 1000, scrollTop: 250, clientHeight: 400 };
snapChatViewportToBottom(detachedViewport);
assert(detachedViewport.scrollTop === 1000, "explicit down snaps to latest");

assert(
  chatScrollTopAfterPrepend({ scrollHeight: 1000, scrollTop: 150 }, 1300) === 450,
  "history prepend adds only the inserted height to the detached viewport",
);
assert(
  chatScrollTopAfterPrepend({ scrollHeight: 1000, scrollTop: 150 }, 1000) === 150,
  "a content update without prepend never moves the detached viewport",
);

clearPersistedChatScrollState(key);
setPersistedChatScrollState(key, { mode: "detached", scrollTop: 315 });
assert(getPersistedChatScrollState(key)?.mode === "detached", "detached mode persists across a remount");
assert(getPersistedChatScrollState(key)?.scrollTop === 315, "detached pixel position persists across a remount");
clearPersistedChatScrollState(key);
assert(getPersistedChatScrollState(key) === null, "clearing removes persisted state");

console.log("chat viewport geometry checks passed");
