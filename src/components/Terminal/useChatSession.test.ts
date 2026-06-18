import {
  TEST_CHAT_REPLY_EVENT,
  installCodeTermTestChatHandle,
  isCodeTermFrontendTestMode,
  resetTestChatClockForCodeTerm,
  testChatReplyPayload,
} from "./useChatSession";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const testWindow = new EventTarget();

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

assert(!isCodeTermFrontendTestMode({ DEV: false, VITE_CODETERM_TEST: "1" }), "release ignores chat test flag");
assert(isCodeTermFrontendTestMode({ DEV: true, VITE_CODETERM_TEST: "1" }), "dev chat test flag enables test mode");

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
window.__codetermTestChat?.reply("pane-a", "from handle");

assert(seen === "from handle", "test chat handle dispatches reply event");

console.log("chat test-mode seam checks passed");
