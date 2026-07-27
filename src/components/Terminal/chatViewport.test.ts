import {
  captureChatScrollAnchor,
  chatActivationRestoreTarget,
  clearPersistedChatScrollState,
  findTopVisibleAnchorId,
  getPersistedChatScrollState,
  isChatViewportAtBottom,
  nextChatViewportMode,
  resolveChatScrollTopForAnchor,
  setPersistedChatScrollState,
  shouldFollowChatSend,
  shouldRestoreChatViewport,
  snapChatViewportToBottom,
} from "./chatViewport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const key = "tab:session";

assert(!shouldRestoreChatViewport(true, false, 0, key, ""), "empty activation waits for messages");
assert(!shouldRestoreChatViewport(true, true, 1, key, ""), "loading activation waits for the session");
assert(shouldRestoreChatViewport(true, false, 1, key, ""), "received messages restore the bottom viewport");
assert(!shouldRestoreChatViewport(true, false, 1, key, key), "one activation restores only once");
assert(!shouldRestoreChatViewport(false, false, 1, key, ""), "hidden chat never scrolls");

assert(isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 600, clientHeight: 400 }), "exact bottom follows messages");
assert(isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 598, clientHeight: 400 }), "subpixel bottom gap follows messages");
assert(!isChatViewportAtBottom({ scrollHeight: 1000, scrollTop: 597, clientHeight: 400 }), "manual scroll detaches immediately");

const awayFromBottom = { scrollHeight: 1200, scrollTop: 600, clientHeight: 400 };
assert(
  nextChatViewportMode(awayFromBottom) === "detached",
  "any viewport observed away from bottom detaches instead of snapping on the next message",
);
assert(
  nextChatViewportMode({ scrollHeight: 1000, scrollTop: 600, clientHeight: 400 }) === "pinned",
  "returning to the bottom re-pins the viewport",
);

const detachedViewport = { scrollHeight: 1000, scrollTop: 250, clientHeight: 400 };
snapChatViewportToBottom(detachedViewport);
assert(detachedViewport.scrollTop === detachedViewport.scrollHeight, "explicit scroll-down action snaps directly to latest");
assert(isChatViewportAtBottom(detachedViewport), "explicit scroll-down action re-enters follow-bottom mode");

assert(shouldFollowChatSend(true, true), "sending while pinned follows the new user message");
assert(!shouldFollowChatSend(false, false), "sending while reviewing history does not move the viewport");
assert(!shouldFollowChatSend(true, false), "manual scrolling during a pending send cancels follow-bottom");

// --- anchor candidate lookup ---

const candidates = [
  { id: "a", offsetTop: 0 },
  { id: "b", offsetTop: 100 },
  { id: "c", offsetTop: 300 },
];
assert(findTopVisibleAnchorId(150, candidates) === "b", "the last candidate at or above scrollTop is the anchor");
assert(findTopVisibleAnchorId(0, candidates) === "a", "scrollTop at the very top anchors the first candidate");
assert(findTopVisibleAnchorId(5000, candidates) === "c", "scrolled past all candidates anchors the last one");
assert(findTopVisibleAnchorId(150, []) === null, "no candidates means no anchor");

// --- capture / resolve round trip ---

const capturedAnchor = captureChatScrollAnchor({ scrollHeight: 1000, scrollTop: 150, clientHeight: 400 }, candidates);
assert(capturedAnchor !== null, "a detached viewport with candidates captures an anchor");
assert(capturedAnchor!.elementId === "b", "the anchor is the element straddling the viewport top");
assert(capturedAnchor!.offsetFromViewportTop === -50, "the anchor records how far its top sits from the viewport top");
assert(
  captureChatScrollAnchor({ scrollHeight: 1000, scrollTop: 150, clientHeight: 400 }, []) === null,
  "capturing with no candidates yields no anchor",
);

assert(
  resolveChatScrollTopForAnchor(capturedAnchor!, 100) === 150,
  "resolving against an unmoved anchor element reproduces the original scrollTop",
);
assert(
  // height change / append: the anchor's own element grew or shifted below it,
  // but the anchor element itself moved down (e.g. a prior sibling grew).
  resolveChatScrollTopForAnchor(capturedAnchor!, 400) === 450,
  "resolving against an anchor pushed down by growth compensates the scrollTop",
);
assert(
  // history prepend: older rows inserted above shift every existing element down
  // by the inserted height (here 300px); the reader's view must shift by exactly that.
  resolveChatScrollTopForAnchor(capturedAnchor!, 100 + 300) === 450,
  "resolving after a history prepend compensates by the inserted height",
);
assert(
  resolveChatScrollTopForAnchor(capturedAnchor!, null) === null,
  "an anchor element that no longer exists cannot be resolved",
);

// --- snap transition: an anchor resolving back to the bottom re-pins ---

// Detached 50px from the bottom (scrollHeight 1000, clientHeight 400 -> bottom at 600).
const nearBottomAnchor = captureChatScrollAnchor({ scrollHeight: 1000, scrollTop: 550, clientHeight: 400 }, candidates);
assert(nearBottomAnchor !== null, "a near-bottom detached viewport still captures an anchor");
assert(nearBottomAnchor!.elementId === "c", "the last candidate anchors a viewport scrolled past it");
// The window trims its oldest visible rows (a, b drop out), sliding every
// remaining offsetTop up by 300px -- the anchor (c) now sits at the very top.
const resolvedAfterTrim = resolveChatScrollTopForAnchor(nearBottomAnchor!, 0);
assert(resolvedAfterTrim === 250, "resolving after a leading trim shifts the scrollTop by the removed height");
assert(
  nextChatViewportMode({ scrollHeight: 650, scrollTop: resolvedAfterTrim!, clientHeight: 400 }) ===
    "pinned",
  "an anchor-resolved scrollTop that lands at the new bottom re-pins the viewport",
);

// --- persisted scroll state survives a remount ---

const persistKey = "tab-1:session-1";
clearPersistedChatScrollState(persistKey);
assert(getPersistedChatScrollState(persistKey) === null, "an unset key has no persisted state");
setPersistedChatScrollState(persistKey, { mode: "detached", anchor: capturedAnchor });
assert(getPersistedChatScrollState(persistKey)?.mode === "detached", "persisted mode survives a get");
assert(getPersistedChatScrollState(persistKey)?.anchor?.elementId === "b", "persisted anchor survives a get");
assert(getPersistedChatScrollState("other-key") === null, "distinct keys do not collide");
clearPersistedChatScrollState(persistKey);
assert(getPersistedChatScrollState(persistKey) === null, "clearing removes the persisted state");

// --- remount restore target: detached anchors restore, everything else snaps to bottom ---

assert(
  chatActivationRestoreTarget(null).mode === "bottom",
  "a fresh Tab with no persisted state restores to the bottom",
);
assert(
  chatActivationRestoreTarget({ mode: "pinned", anchor: null }).mode === "bottom",
  "a pinned Tab restores to the bottom on remount",
);
assert(
  chatActivationRestoreTarget({ mode: "detached", anchor: null }).mode === "bottom",
  "a detached Tab that never captured an anchor falls back to the bottom",
);
const restoreTarget = chatActivationRestoreTarget({ mode: "detached", anchor: capturedAnchor });
assert(restoreTarget.mode === "anchor", "a detached Tab with a captured anchor restores by anchor");
assert(
  restoreTarget.mode === "anchor" && restoreTarget.anchor.elementId === "b",
  "the restore target carries the persisted anchor",
);

console.log("chat viewport activation checks passed");
