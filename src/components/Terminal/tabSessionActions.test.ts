import assert from "node:assert/strict";
import type { Tab } from "../../lib/types";
import { tabSessionActionView } from "./tabSessionActions";

function tab(overrides: Partial<Tab>): Tab {
  return { id: "tab", pty_id: null, tab_type: "Terminal", title: "Tab", cwd: null, ...overrides };
}

assert.deepEqual(
  tabSessionActionView(tab({ agent_type: "claude", agent_session_id: "agent-1", backing: { kind: "pty" } })),
  { sessionId: "agent-1", canReconnect: true, canRestart: true },
);
assert.deepEqual(
  tabSessionActionView(tab({ backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "chat-1" } })),
  { sessionId: "chat-1", canReconnect: true, canRestart: false },
);
assert.deepEqual(
  tabSessionActionView(tab({ backing: { kind: "pty" } })),
  { sessionId: null, canReconnect: false, canRestart: false },
);

console.log("Tab session action tests: passed");
