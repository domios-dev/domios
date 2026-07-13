import {
  TEST_CHAT_REPLY_EVENT,
  installCodeTermTestChatHandle,
  isCodeTermFrontendTestMode,
  listenForTestChatReplies,
  resetTestChatClockForCodeTerm,
  testChatReplyPayload,
} from "./useChatSession";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const testWindow = Object.assign(new EventTarget(), {
  location: {
    protocol: "app:",
    hostname: "",
    port: "",
    search: "?e2e=1",
  },
});

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: testWindow,
});

class TestCustomEvent<T> extends Event {
  detail: T;

  constructor(type: string, init: CustomEventInit<T>) {
    super(type);
    this.detail = init.detail as T;
  }
}

Object.defineProperty(globalThis, "CustomEvent", {
  configurable: true,
  value: TestCustomEvent,
});

assert(!isCodeTermFrontendTestMode({ DEV: false, VITE_CODETERM_TEST: "1" }), "release ignores leaked chat test flag");
assert(!isCodeTermFrontendTestMode({ DEV: false }), "release ignores runtime e2e query");
assert(isCodeTermFrontendTestMode({ DEV: true, VITE_CODETERM_TEST: "1" }), "dev chat test flag enables test mode");
assert(isCodeTermFrontendTestMode({ DEV: true }), "dev runtime e2e query enables test mode");
testWindow.location = { protocol: "http:", hostname: "127.0.0.1", port: "1420", search: "?e2e=1" };
assert(isCodeTermFrontendTestMode({ DEV: false }), "local dev runtime e2e query enables test mode");
testWindow.location = { protocol: "http:", hostname: "127.0.0.1", port: "7685", search: "?e2e=1" };
assert(!isCodeTermFrontendTestMode({ DEV: false }), "prod port ignores runtime e2e query");
testWindow.location = { protocol: "http:", hostname: "127.0.0.1", port: "1420", search: "?e2e=1" };

installCodeTermTestChatHandle(false);
assert(!getTestChatHandle(), "disabled test mode does not install chat handle");

resetTestChatClockForCodeTerm();
const first = testChatReplyPayload({ paneId: "pane-a", content: "deterministic reply" });
const second = testChatReplyPayload({ paneId: "pane-a", content: "next reply" });

assert(first.pane_id === "pane-a", "payload targets pane");
assert(first.messages[0]?.type === "assistant", "default reply is assistant");
assert(first.messages[0]?.content === "deterministic reply", "reply content preserved");
assert(first.messages[0]?.timestamp === "1700000000000", "first timestamp is deterministic");
assert(second.messages[0]?.timestamp === "1700000000001", "clock increments deterministically");

let seen = "";
window.addEventListener(TEST_CHAT_REPLY_EVENT, (event) => {
  seen = (event as CustomEvent<{ content: string }>).detail.content;
});
installCodeTermTestChatHandle(true);
getTestChatHandle()?.reply("pane-a", "from handle");

assert(seen === "from handle", "test chat handle dispatches reply event");

let structured: NormalizedChatMessage[] | null = null;
let order = 0;
getTestChatHandle()?.pushMessages("pane-late", [{
  id: "queued-after-reload",
  type: "assistant",
  content: "queued missing ref pl_missingplan",
  timestamp: "1700000000100",
  provider: "test",
} as any]);
const unlistenLate = listenForTestChatReplies(
  "pane-late",
  (updater) => {
    structured = typeof updater === "function" ? updater(structured) : updater;
  },
  () => order++,
);

const drainedStructured = structured as NormalizedChatMessage[] | null;
assert(
  drainedStructured?.some((message) => message.id === "queued-after-reload" && message.clientOrder === 0),
  "queued test chat message drains when pane listener mounts after reload",
);
unlistenLate?.();

console.log("chat test-mode seam checks passed");

function getTestChatHandle() {
  return window.__codetermTestChat;
}
